use alloy_primitives::{Address as AlloyAddress, B256, Bytes, U256};
use revm::{
    ExecuteEvm, MainBuilder, MainContext,
    bytecode::Bytecode,
    context::{BlockEnv, Context, TxEnv},
    database::CacheDB,
    database_interface::EmptyDB,
    primitives::{Address, TxKind},
    state::AccountInfo,
};
use std::fmt;

const TARGET: Address = Address::new([0xCC; 20]);
const CALLER: Address = Address::new([0xCA; 20]);
const HELPER_ECHO: Address = Address::new([0x11; 20]);
const HELPER_REVERT: Address = Address::new([0x22; 20]);

const HELPER_ECHO_BYTECODE: &[u8] =
    &[0x60, 0x00, 0x35, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xf3];
const HELPER_REVERT_BYTECODE: &[u8] = &[0x60, 0x2a, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xfd];

pub(crate) const HELPER_ECHO_WORD: &str = "0x1111111111111111111111111111111111111111";
pub(crate) const HELPER_REVERT_WORD: &str = "0x2222222222222222222222222222222222222222";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmRunResult {
    pub success: bool,
    pub output: Vec<u8>,
    pub logs: Vec<ObservedLog>,
    pub storage: Vec<ObservedStorageSlot>,
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

pub(crate) fn run_bytecode(
    bytecode: &[u8],
    calldata: &[u8],
) -> Result<EvmRunResult, ExecutionError> {
    let mut db = CacheDB::<EmptyDB>::default();
    db.insert_account_info(
        TARGET,
        AccountInfo::from_bytecode(Bytecode::new_raw(Bytes::copy_from_slice(bytecode))),
    );
    db.insert_account_info(HELPER_ECHO, helper_account(HELPER_ECHO_BYTECODE));
    db.insert_account_info(HELPER_REVERT, helper_account(HELPER_REVERT_BYTECODE));

    let caller =
        AccountInfo { balance: U256::from(1_000_000_000_000_000_000u128), ..Default::default() };
    db.insert_account_info(CALLER, caller);

    let tx = TxEnv::builder()
        .caller(CALLER)
        .kind(TxKind::Call(TARGET))
        .data(Bytes::copy_from_slice(calldata))
        .value(U256::from(7))
        .gas_price(1_000)
        .chain_id(Some(1))
        .build()
        .map_err(|err| ExecutionError::new(format!("invalid transaction environment: {err:?}")))?;

    let mut block = BlockEnv::default();
    block.number = U256::from(12_345);
    block.timestamp = U256::from(1_700_000_001u64);
    block.basefee = 7;

    let result = Context::mainnet()
        .with_db(db)
        .with_block(block)
        .build_mainnet()
        .transact(tx)
        .map_err(|err| ExecutionError::new(format!("EVM transaction failed: {err:?}")))?;

    let success = result.result.is_success();
    let output = result.result.output().map_or_else(Vec::new, |output| output.to_vec());
    let logs = result.result.logs().iter().map(observed_log).collect();
    let storage = result
        .state
        .get(&TARGET)
        .map(|account| {
            let mut slots = account
                .changed_storage_slots()
                .map(|(slot, value)| ObservedStorageSlot {
                    slot: u256_bytes(*slot),
                    value: u256_bytes(value.present_value()),
                })
                .collect::<Vec<_>>();
            slots.sort_by(|left, right| left.slot.cmp(&right.slot));
            slots
        })
        .unwrap_or_default();

    Ok(EvmRunResult { success, output, logs, storage })
}

fn helper_account(bytecode: &[u8]) -> AccountInfo {
    AccountInfo::from_bytecode(Bytecode::new_raw(Bytes::copy_from_slice(bytecode)))
}

fn observed_log(log: &alloy_primitives::Log) -> ObservedLog {
    ObservedLog {
        address: address_bytes(log.address),
        topics: log.data.topics().iter().map(b256_bytes).collect(),
        data: log.data.data.to_vec(),
    }
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
