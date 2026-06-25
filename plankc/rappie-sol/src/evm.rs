use alloy_primitives::{Address as AlloyAddress, B256, Bytes, U256};
use revm::{
    ExecuteCommitEvm, MainBuilder, MainContext,
    bytecode::Bytecode,
    context::{BlockEnv, CfgEnv, Context, TxEnv},
    context_interface::ContextTr,
    database::CacheDB,
    database_interface::EmptyDB,
    primitives::{Address, TxKind, hardfork::SpecId},
    state::AccountInfo,
};
use std::fmt;

const TARGET: Address = Address::new([0xCC; 20]);
const CALLER: Address = Address::new([0xCA; 20]);
const HELPER_ECHO: Address = Address::new([0x11; 20]);
const HELPER_REVERT: Address = Address::new([0x22; 20]);
const HELPER_CODE: Address = Address::new([0x33; 20]);
const EMPTY_ACCOUNT: Address = Address::new([0x44; 20]);
const COINBASE: Address = Address::new([0xCB; 20]);

const HELPER_ECHO_BYTECODE: &[u8] =
    &[0x60, 0x00, 0x35, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xf3];
const HELPER_REVERT_BYTECODE: &[u8] = &[0x60, 0x2a, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xfd];
const HELPER_CODE_BYTECODE: &[u8] = &[0x60, 0x99, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xf3];

pub(crate) const CALLER_WORD: &str = "0xcacacacacacacacacacacacacacacacacacacaca";
pub(crate) const HELPER_ECHO_WORD: &str = "0x1111111111111111111111111111111111111111";
pub(crate) const HELPER_REVERT_WORD: &str = "0x2222222222222222222222222222222222222222";
pub(crate) const HELPER_CODE_WORD: &str = "0x3333333333333333333333333333333333333333";
pub(crate) const EMPTY_ACCOUNT_WORD: &str = "0x4444444444444444444444444444444444444444";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmTrace {
    pub calls: Vec<EvmCallResult>,
    pub final_storage: Vec<ObservedStorageSlot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmCallResult {
    pub success: bool,
    pub output: Vec<u8>,
    pub logs: Vec<ObservedLog>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedLog {
    pub address: [u8; 20],
    pub topics: Vec<[u8; 32]>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedStorageSlot {
    pub slot: [u8; 32],
    pub value: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionError {
    message: String,
}

impl ExecutionError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExecutionError {}

pub(crate) fn run_bytecode_sequence(
    bytecode: &[u8],
    calldatas: &[Vec<u8>],
) -> Result<EvmTrace, ExecutionError> {
    let mut db = CacheDB::<EmptyDB>::default();
    db.insert_account_info(
        TARGET,
        account_with_balance_and_bytecode(
            1_000_000_000_000_000_000u128,
            Bytes::copy_from_slice(bytecode),
        ),
    );
    db.insert_account_info(HELPER_ECHO, helper_account(HELPER_ECHO_BYTECODE));
    db.insert_account_info(HELPER_REVERT, helper_account(HELPER_REVERT_BYTECODE));
    db.insert_account_info(HELPER_CODE, helper_account(HELPER_CODE_BYTECODE));
    db.insert_account_info(EMPTY_ACCOUNT, account_with_balance(0));
    db.insert_account_info(CALLER, account_with_balance(1_000_000_000_000_000_000u128));

    let mut block = BlockEnv {
        number: U256::from(12_345),
        timestamp: U256::from(1_700_000_001u64),
        basefee: 7,
        gas_limit: 1_000_000_000,
        beneficiary: COINBASE,
        difficulty: U256::from(0x1234u64),
        prevrandao: Some(B256::new([0x5a; 32])),
        ..Default::default()
    };
    block.set_blob_excess_gas_and_price(0, 1);

    let mut cfg = CfgEnv::default();
    cfg.set_spec_and_mainnet_gas_params(SpecId::OSAKA);
    // Differential tests compare behavior, not bytecode-specific gas efficiency.
    cfg.tx_gas_limit_cap = Some(u64::MAX);

    let mut evm = Context::mainnet().with_db(db).with_block(block).with_cfg(cfg).build_mainnet();
    let mut calls = Vec::with_capacity(calldatas.len());

    for (index, calldata) in calldatas.iter().enumerate() {
        let tx = TxEnv::builder()
            .caller(CALLER)
            .kind(TxKind::Call(TARGET))
            .nonce(index as u64)
            .data(Bytes::copy_from_slice(calldata))
            .value(U256::from(7))
            .gas_price(1_000)
            .gas_limit(100_000_000)
            .chain_id(Some(1))
            .build()
            .map_err(|err| {
                ExecutionError::new(format!("invalid transaction environment: {err:?}"))
            })?;

        let result = evm
            .transact_commit(tx)
            .map_err(|err| ExecutionError::new(format!("EVM transaction failed: {err:?}")))?;
        calls.push(EvmCallResult {
            success: result.is_success(),
            output: result.output().map_or_else(Vec::new, |output| output.to_vec()),
            logs: result.logs().iter().map(observed_log).collect(),
        });
    }

    Ok(EvmTrace { calls, final_storage: final_storage(evm.ctx.db_ref()) })
}

fn account_with_balance(balance: u128) -> AccountInfo {
    AccountInfo { balance: U256::from(balance), ..Default::default() }
}

fn account_with_balance_and_bytecode(balance: u128, bytecode: Bytes) -> AccountInfo {
    let mut account = AccountInfo::from_bytecode(Bytecode::new_raw(bytecode));
    account.balance = U256::from(balance);
    account
}

fn helper_account(bytecode: &[u8]) -> AccountInfo {
    account_with_balance_and_bytecode(
        1_000_000_000_000_000_000u128,
        Bytes::copy_from_slice(bytecode),
    )
}

fn observed_log(log: &alloy_primitives::Log) -> ObservedLog {
    ObservedLog {
        address: address_bytes(log.address),
        topics: log.data.topics().iter().map(b256_bytes).collect(),
        data: log.data.data.to_vec(),
    }
}

fn final_storage(db: &CacheDB<EmptyDB>) -> Vec<ObservedStorageSlot> {
    let mut slots = db
        .cache
        .accounts
        .get(&TARGET)
        .map(|account| {
            account
                .storage
                .iter()
                .filter(|(_, value)| **value != U256::ZERO)
                .map(|(slot, value)| ObservedStorageSlot {
                    slot: u256_bytes(*slot),
                    value: u256_bytes(*value),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    slots.sort_by_key(|slot| slot.slot);
    slots
}

fn address_bytes(address: AlloyAddress) -> [u8; 20] {
    address.into_array()
}

fn b256_bytes(value: &B256) -> [u8; 32] {
    value.0
}

fn u256_bytes(value: U256) -> [u8; 32] {
    value.to_be_bytes::<32>()
}
