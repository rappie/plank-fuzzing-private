use crate::{
    FuzzCase,
    compiler::{plank::compile_plank_sources, solx::compile_solidity_source},
    evm::{EvmTrace, run_bytecode_sequence},
    sources::PlankSourceSet,
};
use alloy_primitives::hex;
use plank_driver::BackendKind;
use std::fmt;

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
    PlankCompile { backend: &'static str, diagnostics: String },
    SolidityCompile { diagnostics: String },
    PlankExecute { backend: &'static str, message: String },
    SolidityExecute { message: String },
    Mismatch { plank: Box<Execution>, solidity: Box<Execution>, reason: MismatchReason },
}

pub fn compare_plank_solidity(case: &FuzzCase) -> Result<(), HarnessError> {
    let plank_sources = case.plank_sources();
    let solidity_source = case.solidity_source();
    let calldatas = case.calldatas();

    compare_source_set(&plank_sources, &solidity_source, &calldatas)
}

pub fn execute_plank_solidity(
    case: &FuzzCase,
) -> Result<(Vec<Execution>, Execution), HarnessError> {
    let plank_sources = case.plank_sources();
    let solidity_source = case.solidity_source();
    let calldatas = case.calldatas();

    execute_source_set(&plank_sources, &solidity_source, &calldatas)
}

pub fn compare_sources(
    plank_source: &str,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
) -> Result<(), HarnessError> {
    let plank_sources = PlankSourceSet::single_main(plank_source.to_string());
    compare_source_set(&plank_sources, solidity_source, calldatas)
}

pub fn compare_source_set(
    plank_sources: &PlankSourceSet,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
) -> Result<(), HarnessError> {
    let (planks, solidity) = execute_source_set(plank_sources, solidity_source, calldatas)?;

    for plank in planks {
        compare_traces(plank, solidity.clone())?;
    }

    Ok(())
}

fn execute_source_set(
    plank_sources: &PlankSourceSet,
    solidity_source: &str,
    calldatas: &[Vec<u8>],
) -> Result<(Vec<Execution>, Execution), HarnessError> {
    let solidity_bytecode = compile_solidity_source(solidity_source)
        .map_err(|err| HarnessError::SolidityCompile { diagnostics: err.to_string() })?;
    let solidity = Execution {
        name: "solidity",
        trace: run_bytecode_sequence(&solidity_bytecode, calldatas)
            .map_err(|err| HarnessError::SolidityExecute { message: err.to_string() })?,
    };
    let mut planks = Vec::with_capacity(DEFAULT_PLANK_BACKENDS.len());

    for backend in DEFAULT_PLANK_BACKENDS {
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

        planks.push(Execution { name: backend.name, trace });
    }

    Ok((planks, solidity))
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlankCompile { backend, diagnostics } => {
                write!(f, "{backend} compilation failed:\n{diagnostics}")
            }
            Self::SolidityCompile { diagnostics } => {
                write!(f, "Solidity compilation failed:\n{diagnostics}")
            }
            Self::PlankExecute { backend, message } => {
                write!(f, "{backend} execution failed:\n{message}")
            }
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
    use super::{DEFAULT_PLANK_BACKENDS, Execution, HarnessError, MismatchReason, compare_traces};
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
        let solidity = Execution { name: "solidity", trace: trace(vec![call(true, vec![1])]) };

        compare_traces(plank, solidity).expect("matching traces should pass");
    }

    #[test]
    fn compare_traces_rejects_call_count_mismatch() {
        let plank = Execution { name: "sir-debug", trace: trace(vec![call(true, vec![])]) };
        let solidity = Execution { name: "solidity", trace: trace(vec![]) };

        assert!(matches!(
            compare_traces(plank, solidity),
            Err(HarnessError::Mismatch { reason: MismatchReason::CallCount, .. })
        ));
    }

    #[test]
    fn compare_traces_rejects_indexed_success_mismatch() {
        let plank = Execution {
            name: "sir-debug",
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
        let plank = Execution { name: "sir-debug", trace: trace(vec![call(true, vec![1])]) };
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
        let plank = Execution { name: "sir-debug", trace: trace(vec![left]) };
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
        let plank = Execution { name: "sir-debug", trace: plank_trace };
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
