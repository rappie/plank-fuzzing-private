use alloy_primitives::Bytes;
use revm::{
    ExecuteEvm, MainBuilder, MainContext,
    bytecode::Bytecode,
    context::{Context, TxEnv},
    database::CacheDB,
    database_interface::EmptyDB,
    primitives::{Address, TxKind},
    state::AccountInfo,
};
use std::fmt;

const TARGET: Address = Address::new([0xCC; 20]);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmRunResult {
    pub success: bool,
    pub output: Vec<u8>,
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

    let tx = TxEnv::builder()
        .kind(TxKind::Call(TARGET))
        .data(Bytes::copy_from_slice(calldata))
        .build()
        .map_err(|err| ExecutionError::new(format!("invalid transaction environment: {err:?}")))?;

    let result = Context::mainnet()
        .with_db(db)
        .build_mainnet()
        .transact(tx)
        .map_err(|err| ExecutionError::new(format!("EVM transaction failed: {err:?}")))?;

    Ok(EvmRunResult {
        success: result.result.is_success(),
        output: result.result.into_output().map_or_else(Vec::new, |output| output.to_vec()),
    })
}
