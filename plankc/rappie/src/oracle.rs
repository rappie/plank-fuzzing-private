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
    pub optimizations: Option<&'static str>,
}

pub const SIR_DEBUG: BackendSpec =
    BackendSpec { name: "sir-debug", kind: BackendKind::SirDebug, optimizations: None };

pub const SIR_RELEASE: BackendSpec =
    BackendSpec { name: "sir-release", kind: BackendKind::SirRelease, optimizations: None };

pub const SIR_RELEASE_CSUD: BackendSpec = BackendSpec {
    name: "sir-release-csud",
    kind: BackendKind::SirRelease,
    optimizations: Some("csud"),
};

pub const SONA_O0: BackendSpec =
    BackendSpec { name: "sona-o0", kind: BackendKind::Sona, optimizations: Some("O0") };

pub const SONA_O1: BackendSpec =
    BackendSpec { name: "sona-o1", kind: BackendKind::Sona, optimizations: Some("O1") };

pub const SONA_OS: BackendSpec =
    BackendSpec { name: "sona-os", kind: BackendKind::Sona, optimizations: Some("Os") };

pub const SONA_O2: BackendSpec =
    BackendSpec { name: "sona-o2", kind: BackendKind::Sona, optimizations: Some("O2") };

pub const DEFAULT_BACKEND_SET: [BackendSpec; 2] = [SIR_DEBUG, SIR_RELEASE_CSUD];

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
    InvalidBackendSet { backend_count: usize },
    Compile { backend: &'static str, diagnostics: String },
    Execute { backend: &'static str, message: String },
    Mismatch { reference: BackendExecution, candidate: BackendExecution, reason: MismatchReason },
}

pub fn compare_default_backend_set(source: &str, calldata: &[u8]) -> Result<(), HarnessError> {
    compare_backend_set(source, calldata, &DEFAULT_BACKEND_SET)
}

pub fn compare_backend_set(
    source: &str,
    calldata: &[u8],
    backends: &[BackendSpec],
) -> Result<(), HarnessError> {
    let Some((reference, candidates)) = backends.split_first() else {
        return Err(HarnessError::InvalidBackendSet { backend_count: 0 });
    };

    if candidates.is_empty() {
        return Err(HarnessError::InvalidBackendSet { backend_count: 1 });
    }

    let reference_result = compile_and_run(source, calldata, *reference)?;

    for candidate in candidates {
        let candidate_result = compile_and_run(source, calldata, *candidate)?;
        compare_results(reference_result.clone(), candidate_result)?;
    }

    Ok(())
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBackendSet { backend_count } => {
                write!(f, "backend set must contain at least 2 backends, got {backend_count}")
            }
            Self::Compile { backend, diagnostics } => {
                write!(f, "{backend} compilation failed:\n{diagnostics}")
            }
            Self::Execute { backend, message } => {
                write!(f, "{backend} execution failed:\n{message}")
            }
            Self::Mismatch { reference, candidate, reason } => match reason {
                MismatchReason::Success => write!(
                    f,
                    "success mismatch:\nreference {}={}\ncandidate {}={}",
                    reference.backend,
                    reference.result.success,
                    candidate.backend,
                    candidate.result.success
                ),
                MismatchReason::Output => write!(
                    f,
                    "output mismatch:\nreference {}: 0x{}\ncandidate {}: 0x{}",
                    reference.backend,
                    hex::encode(&reference.result.output),
                    candidate.backend,
                    hex::encode(&candidate.result.output)
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
    let bytecode =
        compile_plank_source(source, backend.kind, backend.optimizations).map_err(|err| {
            HarnessError::Compile {
                backend: backend.name,
                diagnostics: err.diagnostics().to_string(),
            }
        })?;

    let result = run_bytecode(&bytecode, calldata)
        .map_err(|err| HarnessError::Execute { backend: backend.name, message: err.to_string() })?;

    Ok(BackendExecution { backend: backend.name, result })
}

fn compare_results(
    reference: BackendExecution,
    candidate: BackendExecution,
) -> Result<(), HarnessError> {
    if reference.result.success != candidate.result.success {
        return Err(HarnessError::Mismatch {
            reference,
            candidate,
            reason: MismatchReason::Success,
        });
    }

    if reference.result.output != candidate.result.output {
        return Err(HarnessError::Mismatch {
            reference,
            candidate,
            reason: MismatchReason::Output,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BackendKind, BackendSpec, DEFAULT_BACKEND_SET, HarnessError, SIR_DEBUG, SIR_RELEASE,
        SIR_RELEASE_CSUD, SONA_O0, SONA_O1, SONA_O2, SONA_OS, compare_backend_set,
    };
    use crate::compiler::compile_plank_source;

    const SMALL_PROGRAM: &str = r#"
init {
    let in0 = @evm_calldataload(0);
    let in1 = @evm_calldataload(32);
    let v0 = @evm_add(in0, in1);
    let b0 = @evm_lt(v0, in0);
    let v1 = if b0 { v0 } else { in1 };

    let out = @malloc_uninit(32);
    @mstore32(out, v1);
    @evm_return(out, 32);
}
"#;

    #[test]
    fn sona_backend_specs_have_distinct_names_and_optimization_levels() {
        assert_eq!(SONA_O0.name, "sona-o0");
        assert_eq!(SONA_O0.optimizations, Some("O0"));
        assert_eq!(SONA_O1.name, "sona-o1");
        assert_eq!(SONA_O1.optimizations, Some("O1"));
        assert_eq!(SONA_OS.name, "sona-os");
        assert_eq!(SONA_OS.optimizations, Some("Os"));
        assert_eq!(SONA_O2.name, "sona-o2");
        assert_eq!(SONA_O2.optimizations, Some("O2"));
    }

    #[test]
    fn sir_release_csud_spec_uses_release_backend_with_csud_passes() {
        assert_eq!(SIR_RELEASE_CSUD.name, "sir-release-csud");
        assert_eq!(SIR_RELEASE_CSUD.kind, BackendKind::SirRelease);
        assert_eq!(SIR_RELEASE_CSUD.optimizations, Some("csud"));
    }

    #[test]
    fn default_backend_set_compares_sir_debug_and_optimized_sir_release() {
        assert_eq!(DEFAULT_BACKEND_SET, [SIR_DEBUG, SIR_RELEASE_CSUD]);
    }

    #[test]
    fn compare_backend_set_rejects_too_few_backends() {
        assert_eq!(
            compare_backend_set(SMALL_PROGRAM, &[], &[]).expect_err("empty set should fail"),
            HarnessError::InvalidBackendSet { backend_count: 0 }
        );

        assert_eq!(
            compare_backend_set(SMALL_PROGRAM, &[], &[SIR_DEBUG])
                .expect_err("single-backend set should fail"),
            HarnessError::InvalidBackendSet { backend_count: 1 }
        );
    }

    #[test]
    fn small_program_compiles_through_default_backends() {
        for backend in DEFAULT_BACKEND_SET {
            compile_plank_source(SMALL_PROGRAM, backend.kind, backend.optimizations)
                .unwrap_or_else(|err| panic!("{} should compile: {err}", backend.name));
        }
    }

    #[test]
    fn compare_backend_set_accepts_matching_default_backends() {
        let calldata = [0u8; 64];

        compare_backend_set(SMALL_PROGRAM, &calldata, &DEFAULT_BACKEND_SET)
            .expect("default backend set should match");
    }

    #[test]
    fn backend_spec_remains_copyable_for_inline_sets() {
        let backends: [BackendSpec; 3] = [SIR_DEBUG, SIR_RELEASE, SONA_O0];

        assert_eq!(backends[0].name, "sir-debug");
        assert_eq!(backends[1].name, "sir-release");
        assert_eq!(backends[2].name, "sona-o0");
    }
}
