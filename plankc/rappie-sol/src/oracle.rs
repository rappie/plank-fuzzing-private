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

pub fn compare_sources(
    plank_source: &str,
    solidity_source: &str,
    calldata: &[u8],
) -> Result<(), HarnessError> {
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

    compare_results(plank, solidity)
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{HarnessError, MismatchReason, compare_results};
    use crate::{EvmRunResult, oracle::Execution};

    #[test]
    fn compare_results_accepts_matching_results() {
        let plank =
            Execution { name: "plank", result: EvmRunResult { success: true, output: vec![1] } };
        let solidity =
            Execution { name: "solidity", result: EvmRunResult { success: true, output: vec![1] } };

        compare_results(plank, solidity).expect("matching results should pass");
    }

    #[test]
    fn compare_results_rejects_success_mismatch() {
        let plank =
            Execution { name: "plank", result: EvmRunResult { success: true, output: vec![] } };
        let solidity =
            Execution { name: "solidity", result: EvmRunResult { success: false, output: vec![] } };

        assert!(matches!(
            compare_results(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::Success, .. })
        ));
    }

    #[test]
    fn compare_results_rejects_output_mismatch() {
        let plank =
            Execution { name: "plank", result: EvmRunResult { success: true, output: vec![1] } };
        let solidity =
            Execution { name: "solidity", result: EvmRunResult { success: true, output: vec![2] } };

        assert!(matches!(
            compare_results(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::Output, .. })
        ));
    }
}
