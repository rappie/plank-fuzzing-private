use crate::{
    compiler::compile_plank_source,
    evm::{EvmRunResult, run_bytecode},
};
use alloy_primitives::hex;
pub use plank_driver::BackendKind;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendSpec {
    pub name: &'static str,
    pub kind: BackendKind,
}

pub const SIR_DEBUG: BackendSpec = BackendSpec { name: "sir-debug", kind: BackendKind::SirDebug };

pub const SIR_RELEASE: BackendSpec =
    BackendSpec { name: "sir-release", kind: BackendKind::SirRelease };

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendExecution {
    pub backend: &'static str,
    pub result: EvmRunResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchReason {
    Success,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessError {
    Compile { backend: &'static str, diagnostics: String },
    Execute { backend: &'static str, message: String },
    Mismatch { left: BackendExecution, right: BackendExecution, reason: MismatchReason },
}

pub fn compare_default_backends(source: &str, calldata: &[u8]) -> Result<(), HarnessError> {
    compare_backends(source, calldata, SIR_DEBUG, SIR_RELEASE)
}

pub fn compare_backends(
    source: &str,
    calldata: &[u8],
    left: BackendSpec,
    right: BackendSpec,
) -> Result<(), HarnessError> {
    let left_result = compile_and_run(source, calldata, left)?;
    let right_result = compile_and_run(source, calldata, right)?;

    compare_results(left_result, right_result)
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compile { backend, diagnostics } => {
                write!(f, "{backend} compilation failed:\n{diagnostics}")
            }
            Self::Execute { backend, message } => {
                write!(f, "{backend} execution failed:\n{message}")
            }
            Self::Mismatch { left, right, reason } => match reason {
                MismatchReason::Success => write!(
                    f,
                    "success mismatch: {}={} {}={}",
                    left.backend, left.result.success, right.backend, right.result.success
                ),
                MismatchReason::Output => write!(
                    f,
                    "output mismatch:\n{}: 0x{}\n{}: 0x{}",
                    left.backend,
                    hex::encode(&left.result.output),
                    right.backend,
                    hex::encode(&right.result.output)
                ),
            },
        }
    }
}

impl std::error::Error for HarnessError {}

fn compile_and_run(
    source: &str,
    calldata: &[u8],
    backend: BackendSpec,
) -> Result<BackendExecution, HarnessError> {
    let bytecode = compile_plank_source(source, backend.kind).map_err(|err| {
        HarnessError::Compile { backend: backend.name, diagnostics: err.diagnostics().to_string() }
    })?;

    let result = run_bytecode(&bytecode, calldata)
        .map_err(|err| HarnessError::Execute { backend: backend.name, message: err.to_string() })?;

    Ok(BackendExecution { backend: backend.name, result })
}

fn compare_results(left: BackendExecution, right: BackendExecution) -> Result<(), HarnessError> {
    if left.result.success != right.result.success {
        return Err(HarnessError::Mismatch { left, right, reason: MismatchReason::Success });
    }

    if left.result.output != right.result.output {
        return Err(HarnessError::Mismatch { left, right, reason: MismatchReason::Output });
    }

    Ok(())
}
