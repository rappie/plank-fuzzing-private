use crate::{
    FuzzCase,
    compiler::{plank::compile_plank_source, solc::compile_solidity_source},
    evm::{EvmRunResult, run_bytecode},
};
use alloy_primitives::hex;
use plank_driver::BackendKind;
use std::fmt;

const PLANK_BACKEND: BackendKind = BackendKind::SirDebug;
const PLANK_OPTIMIZATIONS: Option<&str> = None;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub name: &'static str,
    pub result: EvmRunResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchReason {
    Success,
    Output,
    Logs,
    Storage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessError {
    PlankCompile { diagnostics: String },
    SolidityCompile { diagnostics: String },
    PlankExecute { message: String },
    SolidityExecute { message: String },
    Mismatch { plank: Execution, solidity: Execution, reason: MismatchReason },
}

pub fn compare_plank_solidity(case: &FuzzCase) -> Result<(), HarnessError> {
    let plank_source = case.plank_source();
    let solidity_source = case.solidity_source();
    let calldata = case.calldata();

    compare_sources(&plank_source, &solidity_source, &calldata)
}

pub fn execute_plank_solidity(case: &FuzzCase) -> Result<(Execution, Execution), HarnessError> {
    let plank_source = case.plank_source();
    let solidity_source = case.solidity_source();
    let calldata = case.calldata();

    execute_sources(&plank_source, &solidity_source, &calldata)
}

pub fn compare_sources(
    plank_source: &str,
    solidity_source: &str,
    calldata: &[u8],
) -> Result<(), HarnessError> {
    let (plank, solidity) = execute_sources(plank_source, solidity_source, calldata)?;

    compare_results(plank, solidity)
}

fn execute_sources(
    plank_source: &str,
    solidity_source: &str,
    calldata: &[u8],
) -> Result<(Execution, Execution), HarnessError> {
    let plank_bytecode = compile_plank_source(plank_source, PLANK_BACKEND, PLANK_OPTIMIZATIONS)
        .map_err(|err| HarnessError::PlankCompile { diagnostics: err.diagnostics().to_string() })?;
    let solidity_bytecode = compile_solidity_source(solidity_source)
        .map_err(|err| HarnessError::SolidityCompile { diagnostics: err.to_string() })?;

    let plank = Execution {
        name: "plank",
        result: run_bytecode(&plank_bytecode, calldata)
            .map_err(|err| HarnessError::PlankExecute { message: err.to_string() })?,
    };
    let solidity = Execution {
        name: "solidity",
        result: run_bytecode(&solidity_bytecode, calldata)
            .map_err(|err| HarnessError::SolidityExecute { message: err.to_string() })?,
    };

    Ok((plank, solidity))
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlankCompile { diagnostics } => {
                write!(f, "Plank compilation failed:\n{diagnostics}")
            }
            Self::SolidityCompile { diagnostics } => {
                write!(f, "Solidity compilation failed:\n{diagnostics}")
            }
            Self::PlankExecute { message } => write!(f, "Plank execution failed:\n{message}"),
            Self::SolidityExecute { message } => {
                write!(f, "Solidity execution failed:\n{message}")
            }
            Self::Mismatch { plank, solidity, reason } => match reason {
                MismatchReason::Success => write!(
                    f,
                    "success mismatch:\n{}={}\n{}={}",
                    plank.name, plank.result.success, solidity.name, solidity.result.success
                ),
                MismatchReason::Output => write!(
                    f,
                    "output mismatch:\n{}: 0x{}\n{}: 0x{}",
                    plank.name,
                    hex::encode(&plank.result.output),
                    solidity.name,
                    hex::encode(&solidity.result.output)
                ),
                MismatchReason::Logs => write!(
                    f,
                    "log mismatch:\n{}: {:?}\n{}: {:?}",
                    plank.name, plank.result.logs, solidity.name, solidity.result.logs
                ),
                MismatchReason::Storage => write!(
                    f,
                    "storage mismatch:\n{}: {:?}\n{}: {:?}",
                    plank.name, plank.result.storage, solidity.name, solidity.result.storage
                ),
            },
        }
    }
}

impl std::error::Error for HarnessError {}

fn compare_results(plank: Execution, solidity: Execution) -> Result<(), HarnessError> {
    if plank.result.success != solidity.result.success {
        return Err(HarnessError::Mismatch { plank, solidity, reason: MismatchReason::Success });
    }

    if plank.result.output != solidity.result.output {
        return Err(HarnessError::Mismatch { plank, solidity, reason: MismatchReason::Output });
    }

    if plank.result.logs != solidity.result.logs {
        return Err(HarnessError::Mismatch { plank, solidity, reason: MismatchReason::Logs });
    }

    if plank.result.storage != solidity.result.storage {
        return Err(HarnessError::Mismatch { plank, solidity, reason: MismatchReason::Storage });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{HarnessError, MismatchReason, compare_results};
    use crate::{
        EvmRunResult,
        evm::{ObservedLog, ObservedStorageSlot},
        oracle::Execution,
    };

    #[test]
    fn compare_results_accepts_matching_results() {
        let plank = Execution { name: "plank", result: result(true, vec![1]) };
        let solidity = Execution { name: "solidity", result: result(true, vec![1]) };

        compare_results(plank, solidity).expect("matching results should pass");
    }

    #[test]
    fn compare_results_rejects_success_mismatch() {
        let plank = Execution { name: "plank", result: result(true, vec![]) };
        let solidity = Execution { name: "solidity", result: result(false, vec![]) };

        assert!(matches!(
            compare_results(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::Success, .. })
        ));
    }

    #[test]
    fn compare_results_rejects_output_mismatch() {
        let plank = Execution { name: "plank", result: result(true, vec![1]) };
        let solidity = Execution { name: "solidity", result: result(true, vec![2]) };

        assert!(matches!(
            compare_results(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::Output, .. })
        ));
    }

    #[test]
    fn compare_results_rejects_log_mismatch() {
        let mut left = result(true, vec![1]);
        left.logs.push(ObservedLog { address: [1; 20], topics: vec![[2; 32]], data: vec![3] });
        let plank = Execution { name: "plank", result: left };
        let solidity = Execution { name: "solidity", result: result(true, vec![1]) };

        assert!(matches!(
            compare_results(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::Logs, .. })
        ));
    }

    #[test]
    fn compare_results_rejects_storage_mismatch() {
        let mut left = result(true, vec![1]);
        left.storage.push(ObservedStorageSlot { slot: [1; 32], value: [2; 32] });
        let plank = Execution { name: "plank", result: left };
        let solidity = Execution { name: "solidity", result: result(true, vec![1]) };

        assert!(matches!(
            compare_results(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::Storage, .. })
        ));
    }

    fn result(success: bool, output: Vec<u8>) -> EvmRunResult {
        EvmRunResult { success, output, logs: Vec::new(), storage: Vec::new() }
    }
}
