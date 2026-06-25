use crate::{
    FuzzCase,
    compiler::{plank::compile_plank_source, solx::compile_solidity_source},
    evm::{EvmTrace, run_bytecode_sequence},
};
use alloy_primitives::hex;
use plank_driver::BackendKind;
use std::fmt;

const PLANK_BACKEND: BackendKind = BackendKind::SirDebug;
const PLANK_OPTIMIZATIONS: Option<&str> = None;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub name: &'static str,
    pub trace: EvmTrace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchReason {
    CallCount,
    CallSuccess { index: usize },
    CallOutput { index: usize },
    CallLogs { index: usize },
    FinalStorage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessError {
    PlankCompile { diagnostics: String },
    SolidityCompile { diagnostics: String },
    PlankExecute { message: String },
    SolidityExecute { message: String },
    Mismatch { plank: Box<Execution>, solidity: Box<Execution>, reason: MismatchReason },
}

pub fn compare_plank_solidity(case: &FuzzCase) -> Result<(), HarnessError> {
    let plank_source = case.plank_source();
    let solidity_source = case.solidity_source();
    let calldatas = case.calldatas();

    compare_sources(&plank_source, &solidity_source, &calldatas)
}

pub fn execute_plank_solidity(case: &FuzzCase) -> Result<(Execution, Execution), HarnessError> {
    let plank_source = case.plank_source();
    let solidity_source = case.solidity_source();
    let calldatas = case.calldatas();

    execute_sources(&plank_source, &solidity_source, &calldatas)
}

pub fn compare_sources(
    plank_source: &str,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
) -> Result<(), HarnessError> {
    let (plank, solidity) = execute_sources(plank_source, solidity_source, calldatas)?;

    compare_traces(plank, solidity)
}

fn execute_sources(
    plank_source: &str,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
) -> Result<(Execution, Execution), HarnessError> {
    let plank_bytecode = compile_plank_source(plank_source, PLANK_BACKEND, PLANK_OPTIMIZATIONS)
        .map_err(|err| HarnessError::PlankCompile { diagnostics: err.diagnostics().to_string() })?;
    let solidity_bytecode = compile_solidity_source(solidity_source)
        .map_err(|err| HarnessError::SolidityCompile { diagnostics: err.to_string() })?;

    let plank = Execution {
        name: "plank",
        trace: run_bytecode_sequence(&plank_bytecode, calldatas)
            .map_err(|err| HarnessError::PlankExecute { message: err.to_string() })?,
    };
    let solidity = Execution {
        name: "solidity",
        trace: run_bytecode_sequence(&solidity_bytecode, calldatas)
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
            Self::Mismatch { plank, solidity, reason } => {
                render_mismatch(f, plank, solidity, *reason)
            }
        }
    }
}

impl std::error::Error for HarnessError {}

fn render_mismatch(
    f: &mut fmt::Formatter<'_>,
    plank: &Execution,
    solidity: &Execution,
    reason: MismatchReason,
) -> fmt::Result {
    match reason {
        MismatchReason::CallCount => write!(
            f,
            "call count mismatch:\n{}={}\n{}={}",
            plank.name,
            plank.trace.calls.len(),
            solidity.name,
            solidity.trace.calls.len()
        ),
        MismatchReason::CallSuccess { index } => write!(
            f,
            "call {index} success mismatch:\n{}={}\n{}={}",
            plank.name,
            plank.trace.calls[index].success,
            solidity.name,
            solidity.trace.calls[index].success
        ),
        MismatchReason::CallOutput { index } => write!(
            f,
            "call {index} output mismatch:\n{}: 0x{}\n{}: 0x{}",
            plank.name,
            hex::encode(&plank.trace.calls[index].output),
            solidity.name,
            hex::encode(&solidity.trace.calls[index].output)
        ),
        MismatchReason::CallLogs { index } => write!(
            f,
            "call {index} log mismatch:\n{}: {:?}\n{}: {:?}",
            plank.name,
            plank.trace.calls[index].logs,
            solidity.name,
            solidity.trace.calls[index].logs
        ),
        MismatchReason::FinalStorage => write!(
            f,
            "final storage mismatch:\n{}: {:?}\n{}: {:?}",
            plank.name, plank.trace.final_storage, solidity.name, solidity.trace.final_storage
        ),
    }
}

fn compare_traces(plank: Execution, solidity: Execution) -> Result<(), HarnessError> {
    if plank.trace.calls.len() != solidity.trace.calls.len() {
        return Err(HarnessError::Mismatch {
            plank: Box::new(plank),
            solidity: Box::new(solidity),
            reason: MismatchReason::CallCount,
        });
    }

    for index in 0..plank.trace.calls.len() {
        if plank.trace.calls[index].success != solidity.trace.calls[index].success {
            return Err(HarnessError::Mismatch {
                plank: Box::new(plank),
                solidity: Box::new(solidity),
                reason: MismatchReason::CallSuccess { index },
            });
        }

        if plank.trace.calls[index].output != solidity.trace.calls[index].output {
            return Err(HarnessError::Mismatch {
                plank: Box::new(plank),
                solidity: Box::new(solidity),
                reason: MismatchReason::CallOutput { index },
            });
        }

        if plank.trace.calls[index].logs != solidity.trace.calls[index].logs {
            return Err(HarnessError::Mismatch {
                plank: Box::new(plank),
                solidity: Box::new(solidity),
                reason: MismatchReason::CallLogs { index },
            });
        }
    }

    if plank.trace.final_storage != solidity.trace.final_storage {
        return Err(HarnessError::Mismatch {
            plank: Box::new(plank),
            solidity: Box::new(solidity),
            reason: MismatchReason::FinalStorage,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Execution, HarnessError, MismatchReason, compare_traces};
    use crate::evm::{EvmCallResult, EvmTrace, ObservedLog, ObservedStorageSlot};

    #[test]
    fn compare_traces_accepts_matching_results() {
        let plank = Execution { name: "plank", trace: trace(vec![call(true, vec![1])]) };
        let solidity = Execution { name: "solidity", trace: trace(vec![call(true, vec![1])]) };

        compare_traces(plank, solidity).expect("matching traces should pass");
    }

    #[test]
    fn compare_traces_rejects_call_count_mismatch() {
        let plank = Execution { name: "plank", trace: trace(vec![call(true, vec![])]) };
        let solidity = Execution { name: "solidity", trace: trace(vec![]) };

        assert!(matches!(
            compare_traces(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::CallCount, .. })
        ));
    }

    #[test]
    fn compare_traces_rejects_indexed_success_mismatch() {
        let plank = Execution {
            name: "plank",
            trace: trace(vec![call(true, vec![]), call(false, vec![])]),
        };
        let solidity = Execution {
            name: "solidity",
            trace: trace(vec![call(true, vec![]), call(true, vec![])]),
        };

        assert!(matches!(
            compare_traces(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::CallSuccess { index: 1 }, .. })
        ));
    }

    #[test]
    fn compare_traces_rejects_indexed_output_mismatch() {
        let plank = Execution { name: "plank", trace: trace(vec![call(true, vec![1])]) };
        let solidity = Execution { name: "solidity", trace: trace(vec![call(true, vec![2])]) };

        assert!(matches!(
            compare_traces(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::CallOutput { index: 0 }, .. })
        ));
    }

    #[test]
    fn compare_traces_rejects_indexed_log_mismatch() {
        let mut left = call(true, vec![]);
        left.logs.push(ObservedLog { address: [1; 20], topics: vec![[2; 32]], data: vec![3] });
        let plank = Execution { name: "plank", trace: trace(vec![left]) };
        let solidity = Execution { name: "solidity", trace: trace(vec![call(true, vec![])]) };

        assert!(matches!(
            compare_traces(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::CallLogs { index: 0 }, .. })
        ));
    }

    #[test]
    fn compare_traces_rejects_final_storage_mismatch() {
        let mut plank_trace = trace(vec![call(true, vec![])]);
        plank_trace.final_storage.push(ObservedStorageSlot { slot: [1; 32], value: [2; 32] });
        let plank = Execution { name: "plank", trace: plank_trace };
        let solidity = Execution { name: "solidity", trace: trace(vec![call(true, vec![])]) };

        assert!(matches!(
            compare_traces(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::FinalStorage, .. })
        ));
    }

    fn trace(calls: Vec<EvmCallResult>) -> EvmTrace {
        EvmTrace { calls, final_storage: Vec::new() }
    }

    fn call(success: bool, output: Vec<u8>) -> EvmCallResult {
        EvmCallResult { success, output, logs: Vec::new() }
    }
}
