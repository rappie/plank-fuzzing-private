use crate::{
    BackendConfigError, FuzzCase, OracleBackendSet,
    compiler::{plank::compile_plank_sources, solx::compile_solidity_backend},
    evm::{EvmTrace, run_bytecode_sequence},
    sources::PlankSourceSet,
};
use alloy_primitives::hex;
use plank_driver::BackendKind;
use std::fmt;

pub use crate::compiler::solx::{
    DEFAULT_SOLIDITY_BACKENDS, SOLIDITY_REFERENCE_BACKEND, SolidityBackendSpec,
    SolidityCompilerKind, SolidityOptimizer,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendSpec {
    pub name: &'static str,
    pub kind: BackendKind,
    pub optimizations: Option<&'static str>,
}

pub const DEFAULT_PLANK_BACKENDS: [BackendSpec; 37] = [
    BackendSpec { name: "sir-debug", kind: BackendKind::SirDebug, optimizations: None },
    BackendSpec { name: "sir-release", kind: BackendKind::SirRelease, optimizations: None },
    BackendSpec { name: "sir-release-s", kind: BackendKind::SirRelease, optimizations: Some("s") },
    BackendSpec { name: "sir-release-c", kind: BackendKind::SirRelease, optimizations: Some("c") },
    BackendSpec { name: "sir-release-u", kind: BackendKind::SirRelease, optimizations: Some("u") },
    BackendSpec { name: "sir-release-d", kind: BackendKind::SirRelease, optimizations: Some("d") },
    BackendSpec { name: "sir-release-l", kind: BackendKind::SirRelease, optimizations: Some("l") },
    BackendSpec {
        name: "sir-release-sc",
        kind: BackendKind::SirRelease,
        optimizations: Some("sc"),
    },
    BackendSpec {
        name: "sir-release-su",
        kind: BackendKind::SirRelease,
        optimizations: Some("su"),
    },
    BackendSpec {
        name: "sir-release-sd",
        kind: BackendKind::SirRelease,
        optimizations: Some("sd"),
    },
    BackendSpec {
        name: "sir-release-sl",
        kind: BackendKind::SirRelease,
        optimizations: Some("sl"),
    },
    BackendSpec {
        name: "sir-release-cu",
        kind: BackendKind::SirRelease,
        optimizations: Some("cu"),
    },
    BackendSpec {
        name: "sir-release-cd",
        kind: BackendKind::SirRelease,
        optimizations: Some("cd"),
    },
    BackendSpec {
        name: "sir-release-cl",
        kind: BackendKind::SirRelease,
        optimizations: Some("cl"),
    },
    BackendSpec {
        name: "sir-release-ud",
        kind: BackendKind::SirRelease,
        optimizations: Some("ud"),
    },
    BackendSpec {
        name: "sir-release-ul",
        kind: BackendKind::SirRelease,
        optimizations: Some("ul"),
    },
    BackendSpec {
        name: "sir-release-dl",
        kind: BackendKind::SirRelease,
        optimizations: Some("dl"),
    },
    BackendSpec {
        name: "sir-release-scu",
        kind: BackendKind::SirRelease,
        optimizations: Some("scu"),
    },
    BackendSpec {
        name: "sir-release-scd",
        kind: BackendKind::SirRelease,
        optimizations: Some("scd"),
    },
    BackendSpec {
        name: "sir-release-scl",
        kind: BackendKind::SirRelease,
        optimizations: Some("scl"),
    },
    BackendSpec {
        name: "sir-release-sud",
        kind: BackendKind::SirRelease,
        optimizations: Some("sud"),
    },
    BackendSpec {
        name: "sir-release-sul",
        kind: BackendKind::SirRelease,
        optimizations: Some("sul"),
    },
    BackendSpec {
        name: "sir-release-sdl",
        kind: BackendKind::SirRelease,
        optimizations: Some("sdl"),
    },
    BackendSpec {
        name: "sir-release-cud",
        kind: BackendKind::SirRelease,
        optimizations: Some("cud"),
    },
    BackendSpec {
        name: "sir-release-cul",
        kind: BackendKind::SirRelease,
        optimizations: Some("cul"),
    },
    BackendSpec {
        name: "sir-release-cdl",
        kind: BackendKind::SirRelease,
        optimizations: Some("cdl"),
    },
    BackendSpec {
        name: "sir-release-udl",
        kind: BackendKind::SirRelease,
        optimizations: Some("udl"),
    },
    BackendSpec {
        name: "sir-release-scud",
        kind: BackendKind::SirRelease,
        optimizations: Some("scud"),
    },
    BackendSpec {
        name: "sir-release-scul",
        kind: BackendKind::SirRelease,
        optimizations: Some("scul"),
    },
    BackendSpec {
        name: "sir-release-scdl",
        kind: BackendKind::SirRelease,
        optimizations: Some("scdl"),
    },
    BackendSpec {
        name: "sir-release-sudl",
        kind: BackendKind::SirRelease,
        optimizations: Some("sudl"),
    },
    BackendSpec {
        name: "sir-release-cudl",
        kind: BackendKind::SirRelease,
        optimizations: Some("cudl"),
    },
    BackendSpec {
        name: "sir-release-scudl",
        kind: BackendKind::SirRelease,
        optimizations: Some("scudl"),
    },
    BackendSpec { name: "sona-o0", kind: BackendKind::Sona, optimizations: Some("O0") },
    BackendSpec { name: "sona-o1", kind: BackendKind::Sona, optimizations: Some("O1") },
    BackendSpec { name: "sona-os", kind: BackendKind::Sona, optimizations: Some("Os") },
    BackendSpec { name: "sona-o2", kind: BackendKind::Sona, optimizations: Some("O2") },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub name: &'static str,
    pub trace: EvmTrace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleExecutions {
    pub reference: Execution,
    pub solidity_peers: Vec<Execution>,
    pub plank_backends: Vec<Execution>,
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
    Config(BackendConfigError),
    PlankCompile { backend: &'static str, diagnostics: String },
    SolidityCompile { backend: &'static str, diagnostics: String },
    PlankExecute { backend: &'static str, message: String },
    SolidityExecute { backend: &'static str, message: String },
    Mismatch { candidate: Box<Execution>, reference: Box<Execution>, reason: MismatchReason },
}

pub fn compare_plank_solidity(case: &FuzzCase) -> Result<(), HarnessError> {
    let backends = OracleBackendSet::configured().map_err(HarnessError::Config)?;
    compare_plank_solidity_with_backends(case, backends)
}

pub fn compare_plank_solidity_with_backends(
    case: &FuzzCase,
    backends: &OracleBackendSet,
) -> Result<(), HarnessError> {
    let plank_sources = case.plank_sources();
    let solidity_source = case.solidity_source();
    let calldatas = case.calldatas();

    compare_source_set_with_backends(&plank_sources, &solidity_source, &calldatas, backends)
}

pub fn execute_plank_solidity(case: &FuzzCase) -> Result<OracleExecutions, HarnessError> {
    let backends = OracleBackendSet::configured().map_err(HarnessError::Config)?;
    execute_plank_solidity_with_backends(case, backends)
}

pub fn execute_plank_solidity_with_backends(
    case: &FuzzCase,
    backends: &OracleBackendSet,
) -> Result<OracleExecutions, HarnessError> {
    let plank_sources = case.plank_sources();
    let solidity_source = case.solidity_source();
    let calldatas = case.calldatas();

    execute_source_set(&plank_sources, &solidity_source, &calldatas, backends)
}

pub fn compare_sources(
    plank_source: &str,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
) -> Result<(), HarnessError> {
    let backends = OracleBackendSet::configured().map_err(HarnessError::Config)?;
    compare_sources_with_backends(plank_source, solidity_source, calldatas, backends)
}

pub fn compare_sources_with_backends(
    plank_source: &str,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
    backends: &OracleBackendSet,
) -> Result<(), HarnessError> {
    let plank_sources = PlankSourceSet::single_main(plank_source.to_string());
    compare_source_set_with_backends(&plank_sources, solidity_source, calldatas, backends)
}

pub fn compare_source_set(
    plank_sources: &PlankSourceSet,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
) -> Result<(), HarnessError> {
    let backends = OracleBackendSet::configured().map_err(HarnessError::Config)?;
    compare_source_set_with_backends(plank_sources, solidity_source, calldatas, backends)
}

pub fn compare_source_set_with_backends(
    plank_sources: &PlankSourceSet,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
    backends: &OracleBackendSet,
) -> Result<(), HarnessError> {
    let executions = execute_source_set(plank_sources, solidity_source, calldatas, backends)?;

    for solidity_peer in executions.solidity_peers {
        compare_traces(solidity_peer, executions.reference.clone())?;
    }

    for plank in executions.plank_backends {
        compare_traces(plank, executions.reference.clone())?;
    }

    Ok(())
}

fn execute_source_set(
    plank_sources: &PlankSourceSet,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
    backends: &OracleBackendSet,
) -> Result<OracleExecutions, HarnessError> {
    let reference = execute_solidity_backend(backends.reference, solidity_source, calldatas)?;
    let mut solidity_peers = Vec::with_capacity(backends.solidity_peers.len());

    for &backend in &backends.solidity_peers {
        match execute_solidity_backend(backend, solidity_source, calldatas) {
            Ok(execution) => solidity_peers.push(execution),
            Err(HarnessError::SolidityCompile { diagnostics, .. })
                if is_skippable_solidity_peer_compile_error(backend, &diagnostics) => {}
            Err(err) => return Err(err),
        }
    }

    let mut plank_backends = Vec::with_capacity(backends.plank_backends.len());

    for &backend in &backends.plank_backends {
        let plank_bytecode =
            compile_plank_sources(plank_sources, backend.kind, backend.optimizations).map_err(
                |err| HarnessError::PlankCompile {
                    backend: backend.name,
                    diagnostics: err.diagnostics().to_string(),
                },
            )?;
        let trace = run_bytecode_sequence(&plank_bytecode, calldatas).map_err(|err| {
            HarnessError::PlankExecute { backend: backend.name, message: err.to_string() }
        })?;

        plank_backends.push(Execution { name: backend.name, trace });
    }

    Ok(OracleExecutions { reference, solidity_peers, plank_backends })
}

fn is_skippable_solidity_peer_compile_error(
    backend: SolidityBackendSpec,
    diagnostics: &str,
) -> bool {
    backend.compiler == SolidityCompilerKind::Solc
        && !backend.via_ir
        && diagnostics.contains("Stack too deep")
}

fn execute_solidity_backend(
    backend: SolidityBackendSpec,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
) -> Result<Execution, HarnessError> {
    let bytecode = compile_solidity_backend(solidity_source, backend).map_err(|err| {
        HarnessError::SolidityCompile { backend: backend.name, diagnostics: err.to_string() }
    })?;
    let trace = run_bytecode_sequence(&bytecode, calldatas).map_err(|err| {
        HarnessError::SolidityExecute { backend: backend.name, message: err.to_string() }
    })?;

    Ok(Execution { name: backend.name, trace })
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(err) => write!(f, "backend config failed:\n{err}"),
            Self::PlankCompile { backend, diagnostics } => {
                write!(f, "{backend} compilation failed:\n{diagnostics}")
            }
            Self::SolidityCompile { backend, diagnostics } => {
                write!(f, "{backend} compilation failed:\n{diagnostics}")
            }
            Self::PlankExecute { backend, message } => {
                write!(f, "{backend} execution failed:\n{message}")
            }
            Self::SolidityExecute { backend, message } => {
                write!(f, "{backend} execution failed:\n{message}")
            }
            Self::Mismatch { candidate, reference, reason } => {
                render_mismatch(f, candidate, reference, *reason)
            }
        }
    }
}

impl std::error::Error for HarnessError {}

fn render_mismatch(
    f: &mut fmt::Formatter<'_>,
    candidate: &Execution,
    reference: &Execution,
    reason: MismatchReason,
) -> fmt::Result {
    match reason {
        MismatchReason::CallCount => write!(
            f,
            "call count mismatch:\n{}={}\n{}={}",
            candidate.name,
            candidate.trace.calls.len(),
            reference.name,
            reference.trace.calls.len()
        ),
        MismatchReason::CallSuccess { index } => write!(
            f,
            "call {index} success mismatch:\n{}={}\n{}={}",
            candidate.name,
            candidate.trace.calls[index].success,
            reference.name,
            reference.trace.calls[index].success
        ),
        MismatchReason::CallOutput { index } => write!(
            f,
            "call {index} output mismatch:\n{}: 0x{}\n{}: 0x{}",
            candidate.name,
            hex::encode(&candidate.trace.calls[index].output),
            reference.name,
            hex::encode(&reference.trace.calls[index].output)
        ),
        MismatchReason::CallLogs { index } => write!(
            f,
            "call {index} log mismatch:\n{}: {:?}\n{}: {:?}",
            candidate.name,
            candidate.trace.calls[index].logs,
            reference.name,
            reference.trace.calls[index].logs
        ),
        MismatchReason::FinalStorage => write!(
            f,
            "final storage mismatch:\n{}: {:?}\n{}: {:?}",
            candidate.name,
            candidate.trace.final_storage,
            reference.name,
            reference.trace.final_storage
        ),
    }
}

fn compare_traces(candidate: Execution, reference: Execution) -> Result<(), HarnessError> {
    if candidate.trace.calls.len() != reference.trace.calls.len() {
        return Err(HarnessError::Mismatch {
            candidate: Box::new(candidate),
            reference: Box::new(reference),
            reason: MismatchReason::CallCount,
        });
    }

    for index in 0..candidate.trace.calls.len() {
        if candidate.trace.calls[index].success != reference.trace.calls[index].success {
            return Err(HarnessError::Mismatch {
                candidate: Box::new(candidate),
                reference: Box::new(reference),
                reason: MismatchReason::CallSuccess { index },
            });
        }

        if candidate.trace.calls[index].output != reference.trace.calls[index].output {
            return Err(HarnessError::Mismatch {
                candidate: Box::new(candidate),
                reference: Box::new(reference),
                reason: MismatchReason::CallOutput { index },
            });
        }

        if candidate.trace.calls[index].logs != reference.trace.calls[index].logs {
            return Err(HarnessError::Mismatch {
                candidate: Box::new(candidate),
                reference: Box::new(reference),
                reason: MismatchReason::CallLogs { index },
            });
        }
    }

    if candidate.trace.final_storage != reference.trace.final_storage {
        return Err(HarnessError::Mismatch {
            candidate: Box::new(candidate),
            reference: Box::new(reference),
            reason: MismatchReason::FinalStorage,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_PLANK_BACKENDS, DEFAULT_SOLIDITY_BACKENDS, Execution, HarnessError, MismatchReason,
        SolidityCompilerKind, SolidityOptimizer, compare_traces,
    };
    use crate::{
        compiler::plank::compile_plank_sources,
        evm::{EvmCallResult, EvmTrace, ObservedLog, ObservedStorageSlot},
        sources::PlankSourceSet,
    };
    use plank_driver::BackendKind;
    use std::collections::BTreeSet;

    const SMALL_PROGRAM: &str = r#"
init {
    @evm_stop();
}
"#;

    #[test]
    fn default_backend_set_has_expected_size_and_unique_names() {
        assert_eq!(DEFAULT_PLANK_BACKENDS.len(), 37);

        let mut names = BTreeSet::new();
        for backend in DEFAULT_PLANK_BACKENDS {
            assert!(names.insert(backend.name), "duplicate backend name {}", backend.name);
        }
    }

    #[test]
    fn default_solidity_backend_set_has_expected_matrix_and_unique_names() {
        assert_eq!(DEFAULT_SOLIDITY_BACKENDS.len(), 4);

        let mut names = BTreeSet::new();
        for backend in DEFAULT_SOLIDITY_BACKENDS {
            assert!(names.insert(backend.name), "duplicate backend name {}", backend.name);
        }

        let actual = DEFAULT_SOLIDITY_BACKENDS
            .iter()
            .map(|backend| (backend.name, backend.compiler, backend.optimizer, backend.via_ir))
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            vec![
                (
                    "solc-noopt-legacy",
                    SolidityCompilerKind::Solc,
                    SolidityOptimizer::Disabled,
                    false,
                ),
                (
                    "solc-noopt-via-ir",
                    SolidityCompilerKind::Solc,
                    SolidityOptimizer::Disabled,
                    true,
                ),
                (
                    "solc-opt-legacy",
                    SolidityCompilerKind::Solc,
                    SolidityOptimizer::Enabled { runs: 200 },
                    false,
                ),
                (
                    "solc-opt-via-ir",
                    SolidityCompilerKind::Solc,
                    SolidityOptimizer::Enabled { runs: 200 },
                    true,
                ),
            ]
        );
    }

    #[test]
    fn default_backend_set_contains_sir_release_optimization_subsets_in_scudl_order() {
        let actual = DEFAULT_PLANK_BACKENDS
            .iter()
            .filter(|backend| backend.kind == BackendKind::SirRelease)
            .filter_map(|backend| backend.optimizations)
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            vec![
                "s", "c", "u", "d", "l", "sc", "su", "sd", "sl", "cu", "cd", "cl", "ud", "ul",
                "dl", "scu", "scd", "scl", "sud", "sul", "sdl", "cud", "cul", "cdl", "udl", "scud",
                "scul", "scdl", "sudl", "cudl", "scudl",
            ]
        );
    }

    #[test]
    fn default_backend_set_contains_sona_optimization_levels() {
        let actual = DEFAULT_PLANK_BACKENDS
            .iter()
            .filter(|backend| backend.kind == BackendKind::Sona)
            .map(|backend| (backend.name, backend.optimizations))
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            vec![
                ("sona-o0", Some("O0")),
                ("sona-o1", Some("O1")),
                ("sona-os", Some("Os")),
                ("sona-o2", Some("O2")),
            ]
        );
    }

    #[test]
    fn small_program_compiles_through_every_default_backend() {
        let sources = PlankSourceSet::single_main(SMALL_PROGRAM.to_string());

        for backend in DEFAULT_PLANK_BACKENDS {
            compile_plank_sources(&sources, backend.kind, backend.optimizations)
                .unwrap_or_else(|err| panic!("{} should compile: {err}", backend.name));
        }
    }

    #[test]
    fn compare_traces_accepts_matching_results() {
        let plank = Execution { name: "sir-debug", trace: trace(vec![call(true, vec![1])]) };
        let solidity =
            Execution { name: "solx-reference", trace: trace(vec![call(true, vec![1])]) };

        compare_traces(plank, solidity).expect("matching traces should pass");
    }

    #[test]
    fn compare_traces_rejects_call_count_mismatch() {
        let plank = Execution { name: "sir-debug", trace: trace(vec![call(true, vec![])]) };
        let solidity = Execution { name: "solx-reference", trace: trace(vec![]) };

        assert!(matches!(
            compare_traces(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::CallCount, .. })
        ));
    }

    #[test]
    fn solc_legacy_stack_too_deep_is_skippable_for_peer_backends() {
        assert!(super::is_skippable_solidity_peer_compile_error(
            super::DEFAULT_SOLIDITY_BACKENDS[0],
            "CompilerError: Stack too deep."
        ));
        assert!(super::is_skippable_solidity_peer_compile_error(
            super::DEFAULT_SOLIDITY_BACKENDS[2],
            "CompilerError: Stack too deep. Try compiling with `--via-ir`."
        ));
    }

    #[test]
    fn solc_via_ir_and_other_diagnostics_are_not_skippable() {
        assert!(!super::is_skippable_solidity_peer_compile_error(
            super::DEFAULT_SOLIDITY_BACKENDS[1],
            "CompilerError: Stack too deep."
        ));
        assert!(!super::is_skippable_solidity_peer_compile_error(
            super::DEFAULT_SOLIDITY_BACKENDS[3],
            "CompilerError: Stack too deep."
        ));
        assert!(!super::is_skippable_solidity_peer_compile_error(
            super::DEFAULT_SOLIDITY_BACKENDS[0],
            "InternalCompilerError: badness"
        ));
    }

    #[test]
    fn compare_traces_rejects_indexed_success_mismatch() {
        let plank = Execution {
            name: "sir-debug",
            trace: trace(vec![call(true, vec![]), call(false, vec![])]),
        };
        let solidity = Execution {
            name: "solx-reference",
            trace: trace(vec![call(true, vec![]), call(true, vec![])]),
        };

        assert!(matches!(
            compare_traces(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::CallSuccess { index: 1 }, .. })
        ));
    }

    #[test]
    fn compare_traces_rejects_indexed_output_mismatch() {
        let plank = Execution { name: "sir-debug", trace: trace(vec![call(true, vec![1])]) };
        let solidity =
            Execution { name: "solx-reference", trace: trace(vec![call(true, vec![2])]) };

        assert!(matches!(
            compare_traces(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::CallOutput { index: 0 }, .. })
        ));
    }

    #[test]
    fn compare_traces_rejects_indexed_log_mismatch() {
        let mut left = call(true, vec![]);
        left.logs.push(ObservedLog { address: [1; 20], topics: vec![[2; 32]], data: vec![3] });
        let plank = Execution { name: "sir-debug", trace: trace(vec![left]) };
        let solidity = Execution { name: "solx-reference", trace: trace(vec![call(true, vec![])]) };

        assert!(matches!(
            compare_traces(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::CallLogs { index: 0 }, .. })
        ));
    }

    #[test]
    fn compare_traces_rejects_final_storage_mismatch() {
        let mut plank_trace = trace(vec![call(true, vec![])]);
        plank_trace.final_storage.push(ObservedStorageSlot { slot: [1; 32], value: [2; 32] });
        let plank = Execution { name: "sir-debug", trace: plank_trace };
        let solidity = Execution { name: "solx-reference", trace: trace(vec![call(true, vec![])]) };

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
