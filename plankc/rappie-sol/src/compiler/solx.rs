use alloy_primitives::hex;
use serde_json::{Map, Value, json};
use std::{
    env, fmt,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

const MAIN_SOURCE: &str = "main.sol";
const CONTRACT_NAME: &str = "C";
const EVM_VERSION_FALLBACKS: [&str; 4] = ["osaka", "prague", "cancun", "shanghai"];
const SOLX_ARGS: [&str; 3] = ["--standard-json", "--threads", "1"];
const SOLC_ARGS: [&str; 1] = ["--standard-json"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolidityCompilerKind {
    Solx,
    Solc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolidityOptimizer {
    Disabled,
    Enabled { runs: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolidityBackendSpec {
    pub name: &'static str,
    pub compiler: SolidityCompilerKind,
    pub optimizer: SolidityOptimizer,
    pub via_ir: bool,
}

pub const SOLIDITY_REFERENCE_BACKEND: SolidityBackendSpec = SolidityBackendSpec {
    name: "solx-reference",
    compiler: SolidityCompilerKind::Solx,
    optimizer: SolidityOptimizer::Disabled,
    via_ir: false,
};

pub const DEFAULT_SOLIDITY_BACKENDS: [SolidityBackendSpec; 4] = [
    SolidityBackendSpec {
        name: "solc-noopt-legacy",
        compiler: SolidityCompilerKind::Solc,
        optimizer: SolidityOptimizer::Disabled,
        via_ir: false,
    },
    SolidityBackendSpec {
        name: "solc-noopt-via-ir",
        compiler: SolidityCompilerKind::Solc,
        optimizer: SolidityOptimizer::Disabled,
        via_ir: true,
    },
    SolidityBackendSpec {
        name: "solc-opt-legacy",
        compiler: SolidityCompilerKind::Solc,
        optimizer: SolidityOptimizer::Enabled { runs: 200 },
        via_ir: false,
    },
    SolidityBackendSpec {
        name: "solc-opt-via-ir",
        compiler: SolidityCompilerKind::Solc,
        optimizer: SolidityOptimizer::Enabled { runs: 200 },
        via_ir: true,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompileError {
    CompilerUnavailable { backend: &'static str, path: PathBuf, message: String },
    ProcessFailed { backend: &'static str, status: String, stderr: String },
    InvalidJson { backend: &'static str, message: String, stdout: String, stderr: String },
    CompilerDiagnostics { diagnostics: String },
    MissingBytecode { message: String },
    InvalidBytecode { message: String },
    StdinUnavailable { backend: &'static str },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompilerUnavailable { backend, path, message } => {
                write!(f, "could not execute {backend} at {}: {message}", path.display())
            }
            Self::ProcessFailed { backend, status, stderr } => {
                write!(f, "{backend} exited with {status}:\n{stderr}")
            }
            Self::InvalidJson { backend, message, stdout, stderr } => {
                write!(
                    f,
                    "{backend} produced invalid JSON: {message}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                )
            }
            Self::CompilerDiagnostics { diagnostics } => f.write_str(diagnostics),
            Self::MissingBytecode { message } | Self::InvalidBytecode { message } => {
                f.write_str(message)
            }
            Self::StdinUnavailable { backend } => write!(f, "{backend} stdin was unavailable"),
        }
    }
}

impl std::error::Error for CompileError {}

pub(crate) fn compile_solidity_backend(
    source: &str,
    backend: SolidityBackendSpec,
) -> Result<Vec<u8>, CompileError> {
    let compiler = resolve_compiler(backend.compiler);
    let mut last_invalid_evm_version = None;

    for evm_version in EVM_VERSION_FALLBACKS {
        match compile_with_evm_version(source, backend, &compiler, evm_version) {
            Ok(bytecode) => return Ok(bytecode),
            Err(err) if is_invalid_evm_version(&err) => {
                last_invalid_evm_version = Some(err);
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_invalid_evm_version.unwrap_or_else(|| CompileError::CompilerDiagnostics {
        diagnostics: format!("{} did not accept any configured EVM version", backend.name),
    }))
}

fn resolve_compiler(kind: SolidityCompilerKind) -> PathBuf {
    match kind {
        SolidityCompilerKind::Solx => env::var_os("RAPPIE_SOL_SOLX")
            .or_else(|| env::var_os("SOLX_PATH"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("solx")),
        SolidityCompilerKind::Solc => env::var_os("RAPPIE_SOL_SOLC")
            .or_else(|| env::var_os("SOLC_PATH"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("solc")),
    }
}

fn compile_with_evm_version(
    source: &str,
    backend: SolidityBackendSpec,
    compiler: &PathBuf,
    evm_version: &str,
) -> Result<Vec<u8>, CompileError> {
    let input = standard_json_input(source, evm_version, backend);
    let input = serde_json::to_vec(&input).expect("standard JSON input should serialize");

    let mut child = Command::new(compiler)
        .args(compiler_args(backend.compiler))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| CompileError::CompilerUnavailable {
            backend: backend.name,
            path: compiler.clone(),
            message: err.to_string(),
        })?;

    let mut stdin =
        child.stdin.take().ok_or(CompileError::StdinUnavailable { backend: backend.name })?;
    stdin.write_all(&input).map_err(|err| CompileError::ProcessFailed {
        backend: backend.name,
        status: "while writing stdin".to_string(),
        stderr: err.to_string(),
    })?;
    drop(stdin);

    let output = child.wait_with_output().map_err(|err| CompileError::ProcessFailed {
        backend: backend.name,
        status: format!("while waiting for {}", backend.name),
        stderr: err.to_string(),
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let parsed = serde_json::from_str::<Value>(&stdout).map_err(|err| {
        if output.status.success() {
            CompileError::InvalidJson {
                backend: backend.name,
                message: err.to_string(),
                stdout,
                stderr: stderr.clone(),
            }
        } else {
            CompileError::ProcessFailed {
                backend: backend.name,
                status: output.status.to_string(),
                stderr: stderr.clone(),
            }
        }
    })?;

    if let Some(diagnostics) = compiler_error_diagnostics(&parsed) {
        return Err(CompileError::CompilerDiagnostics { diagnostics });
    }

    if !output.status.success() {
        return Err(CompileError::ProcessFailed {
            backend: backend.name,
            status: output.status.to_string(),
            stderr,
        });
    }

    let bytecode = deployed_bytecode(&parsed).ok_or_else(|| CompileError::MissingBytecode {
        message: format!(
            "{} output did not contain contracts.{MAIN_SOURCE}.{CONTRACT_NAME}.evm.deployedBytecode.object",
            backend.name
        ),
    })?;

    if bytecode.is_empty() {
        return Err(CompileError::MissingBytecode {
            message: format!("{} produced empty deployed bytecode", backend.name),
        });
    }

    hex::decode(bytecode.strip_prefix("0x").unwrap_or(bytecode)).map_err(|err| {
        CompileError::InvalidBytecode {
            message: format!("{} produced invalid hex bytecode: {err}", backend.name),
        }
    })
}

fn compiler_args(kind: SolidityCompilerKind) -> &'static [&'static str] {
    match kind {
        SolidityCompilerKind::Solx => solx_args(),
        SolidityCompilerKind::Solc => solc_args(),
    }
}

fn solx_args() -> &'static [&'static str] {
    &SOLX_ARGS
}

fn solc_args() -> &'static [&'static str] {
    &SOLC_ARGS
}

fn standard_json_input(source: &str, evm_version: &str, backend: SolidityBackendSpec) -> Value {
    let mut sources = Map::new();
    sources.insert(MAIN_SOURCE.to_string(), json!({ "content": source }));

    let mut settings = Map::new();
    settings.insert("evmVersion".to_string(), json!(evm_version));
    settings.insert(
        "metadata".to_string(),
        json!({
            "appendCBOR": false,
            "bytecodeHash": "none",
        }),
    );
    settings.insert(
        "outputSelection".to_string(),
        json!({
            "*": {
                "*": ["evm.deployedBytecode.object"],
            },
        }),
    );

    if backend.compiler == SolidityCompilerKind::Solc {
        settings.insert(
            "optimizer".to_string(),
            match backend.optimizer {
                SolidityOptimizer::Disabled => json!({ "enabled": false }),
                SolidityOptimizer::Enabled { runs } => json!({ "enabled": true, "runs": runs }),
            },
        );
        settings.insert("viaIR".to_string(), json!(backend.via_ir));
    }

    json!({
        "language": "Solidity",
        "sources": sources,
        "settings": settings,
    })
}

fn compiler_error_diagnostics(output: &Value) -> Option<String> {
    let errors = output.get("errors")?.as_array()?;
    let has_error =
        errors.iter().any(|error| error.get("severity").and_then(Value::as_str) == Some("error"));

    has_error.then(|| render_compiler_errors(errors))
}

fn render_compiler_errors(errors: &[Value]) -> String {
    errors
        .iter()
        .map(|error| {
            error
                .get("formattedMessage")
                .and_then(Value::as_str)
                .or_else(|| error.get("message").and_then(Value::as_str))
                .unwrap_or("Solidity compiler reported an error without a message")
                .trim()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n----\n")
}

fn deployed_bytecode(output: &Value) -> Option<&str> {
    output
        .get("contracts")?
        .get(MAIN_SOURCE)?
        .get(CONTRACT_NAME)?
        .get("evm")?
        .get("deployedBytecode")?
        .get("object")?
        .as_str()
}

fn is_invalid_evm_version(err: &CompileError) -> bool {
    let CompileError::CompilerDiagnostics { diagnostics } = err else {
        return false;
    };

    diagnostics.contains("Invalid EVM version")
        || diagnostics.contains("Invalid evmVersion")
        || diagnostics.contains("invalid evmVersion")
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_SOLIDITY_BACKENDS, SOLIDITY_REFERENCE_BACKEND, SolidityOptimizer,
        compile_solidity_backend, solc_args, solx_args, standard_json_input,
    };

    const FALLBACK_SOURCE: &str = r#"
// SPDX-License-Identifier: MIT
pragma solidity >=0.8.20;

contract C {
    fallback() external payable {
        assembly ("memory-safe") {
            mstore(0, calldataload(0))
            return(0, 32)
        }
    }
}
"#;

    #[test]
    fn standard_json_input_embeds_source_content() {
        let input = standard_json_input(FALLBACK_SOURCE, "osaka", SOLIDITY_REFERENCE_BACKEND);

        assert_eq!(input["language"], "Solidity");
        assert_eq!(input["sources"]["main.sol"]["content"], FALLBACK_SOURCE);
        assert_eq!(input["settings"]["evmVersion"], "osaka");
        assert_eq!(input["settings"]["metadata"]["appendCBOR"], false);
        assert_eq!(input["settings"]["metadata"]["bytecodeHash"], "none");
        assert!(input["settings"].get("optimizer").is_none());
        assert!(input["settings"].get("viaIR").is_none());
    }

    #[test]
    fn standard_json_input_configures_solc_noopt_legacy() {
        let input = standard_json_input(FALLBACK_SOURCE, "prague", DEFAULT_SOLIDITY_BACKENDS[0]);

        assert_eq!(input["settings"]["optimizer"]["enabled"], false);
        assert!(input["settings"]["optimizer"].get("runs").is_none());
        assert_eq!(input["settings"]["viaIR"], false);
        assert_eq!(input["settings"]["metadata"]["appendCBOR"], false);
        assert_eq!(
            input["settings"]["outputSelection"]["*"]["*"][0],
            "evm.deployedBytecode.object"
        );
    }

    #[test]
    fn standard_json_input_configures_solc_opt_via_ir() {
        let input = standard_json_input(FALLBACK_SOURCE, "prague", DEFAULT_SOLIDITY_BACKENDS[3]);

        assert_eq!(
            DEFAULT_SOLIDITY_BACKENDS[3].optimizer,
            SolidityOptimizer::Enabled { runs: 200 }
        );
        assert_eq!(input["settings"]["optimizer"]["enabled"], true);
        assert_eq!(input["settings"]["optimizer"]["runs"], 200);
        assert_eq!(input["settings"]["viaIR"], true);
    }

    #[test]
    fn solx_args_pin_one_compiler_thread() {
        assert_eq!(solx_args(), ["--standard-json", "--threads", "1"]);
    }

    #[test]
    fn solc_args_use_standard_json_only() {
        assert_eq!(solc_args(), ["--standard-json"]);
    }

    #[test]
    #[ignore = "requires RAPPIE_SOL_SOLX, SOLX_PATH, or solx on PATH"]
    fn compiles_minimal_fallback_contract() {
        let bytecode = compile_solidity_backend(FALLBACK_SOURCE, SOLIDITY_REFERENCE_BACKEND)
            .expect("fallback source should compile");

        assert!(!bytecode.is_empty());
    }

    #[test]
    #[ignore = "requires RAPPIE_SOL_SOLC, SOLC_PATH, or solc on PATH"]
    fn compiles_minimal_fallback_contract_through_solc_matrix() {
        for backend in DEFAULT_SOLIDITY_BACKENDS {
            let bytecode = compile_solidity_backend(FALLBACK_SOURCE, backend)
                .unwrap_or_else(|err| panic!("{} should compile: {err}", backend.name));

            assert!(!bytecode.is_empty(), "{} produced empty bytecode", backend.name);
        }
    }
}
