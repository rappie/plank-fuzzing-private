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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompileError {
    SolcUnavailable { path: PathBuf, message: String },
    ProcessFailed { status: String, stderr: String },
    InvalidJson { message: String, stdout: String, stderr: String },
    CompilerDiagnostics { diagnostics: String },
    MissingBytecode { message: String },
    InvalidBytecode { message: String },
    StdinUnavailable,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SolcUnavailable { path, message } => {
                write!(f, "could not execute solc at {}: {message}", path.display())
            }
            Self::ProcessFailed { status, stderr } => {
                write!(f, "solc exited with {status}:\n{stderr}")
            }
            Self::InvalidJson { message, stdout, stderr } => {
                write!(
                    f,
                    "solc produced invalid JSON: {message}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                )
            }
            Self::CompilerDiagnostics { diagnostics } => f.write_str(diagnostics),
            Self::MissingBytecode { message } | Self::InvalidBytecode { message } => {
                f.write_str(message)
            }
            Self::StdinUnavailable => f.write_str("solc stdin was unavailable"),
        }
    }
}

impl std::error::Error for CompileError {}

pub(crate) fn compile_solidity_source(source: &str) -> Result<Vec<u8>, CompileError> {
    let solc = resolve_solc();
    let mut last_invalid_evm_version = None;

    for evm_version in EVM_VERSION_FALLBACKS {
        match compile_with_evm_version(source, &solc, evm_version) {
            Ok(bytecode) => return Ok(bytecode),
            Err(err) if is_invalid_evm_version(&err) => {
                last_invalid_evm_version = Some(err);
            }
            Err(err) => return Err(err),
        }
    }

    Err(last_invalid_evm_version.unwrap_or_else(|| CompileError::CompilerDiagnostics {
        diagnostics: "solc did not accept any configured EVM version".to_string(),
    }))
}

fn resolve_solc() -> PathBuf {
    env::var_os("RAPPIE_SOL_SOLC")
        .or_else(|| env::var_os("SOLC_PATH"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("solc"))
}

fn compile_with_evm_version(
    source: &str,
    solc: &PathBuf,
    evm_version: &str,
) -> Result<Vec<u8>, CompileError> {
    let input = standard_json_input(source, evm_version);
    let input = serde_json::to_vec(&input).expect("standard JSON input should serialize");

    let mut child = Command::new(solc)
        .arg("--standard-json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| CompileError::SolcUnavailable {
            path: solc.clone(),
            message: err.to_string(),
        })?;

    let mut stdin = child.stdin.take().ok_or(CompileError::StdinUnavailable)?;
    stdin.write_all(&input).map_err(|err| CompileError::ProcessFailed {
        status: "while writing stdin".to_string(),
        stderr: err.to_string(),
    })?;
    drop(stdin);

    let output = child.wait_with_output().map_err(|err| CompileError::ProcessFailed {
        status: "while waiting for solc".to_string(),
        stderr: err.to_string(),
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let parsed = serde_json::from_str::<Value>(&stdout).map_err(|err| {
        if output.status.success() {
            CompileError::InvalidJson { message: err.to_string(), stdout, stderr: stderr.clone() }
        } else {
            CompileError::ProcessFailed {
                status: output.status.to_string(),
                stderr: stderr.clone(),
            }
        }
    })?;

    if let Some(diagnostics) = compiler_error_diagnostics(&parsed) {
        return Err(CompileError::CompilerDiagnostics { diagnostics });
    }

    if !output.status.success() {
        return Err(CompileError::ProcessFailed { status: output.status.to_string(), stderr });
    }

    let bytecode = deployed_bytecode(&parsed).ok_or_else(|| CompileError::MissingBytecode {
        message: format!(
            "solc output did not contain contracts.{MAIN_SOURCE}.{CONTRACT_NAME}.evm.deployedBytecode.object"
        ),
    })?;

    if bytecode.is_empty() {
        return Err(CompileError::MissingBytecode {
            message: "solc produced empty deployed bytecode".to_string(),
        });
    }

    hex::decode(bytecode.strip_prefix("0x").unwrap_or(bytecode)).map_err(|err| {
        CompileError::InvalidBytecode {
            message: format!("solc produced invalid hex bytecode: {err}"),
        }
    })
}

fn standard_json_input(source: &str, evm_version: &str) -> Value {
    let mut sources = Map::new();
    sources.insert(MAIN_SOURCE.to_string(), json!({ "content": source }));

    json!({
        "language": "Solidity",
        "sources": sources,
        "settings": {
            "evmVersion": evm_version,
            "metadata": {
                "appendCBOR": false,
                "bytecodeHash": "none",
            },
            "outputSelection": {
                "*": {
                    "*": ["evm.deployedBytecode.object"],
                },
            },
        },
    })
}

fn compiler_error_diagnostics(output: &Value) -> Option<String> {
    let errors = output.get("errors")?.as_array()?;
    let has_error =
        errors.iter().any(|error| error.get("severity").and_then(Value::as_str) == Some("error"));

    has_error.then(|| render_solc_errors(errors))
}

fn render_solc_errors(errors: &[Value]) -> String {
    errors
        .iter()
        .map(|error| {
            error
                .get("formattedMessage")
                .and_then(Value::as_str)
                .or_else(|| error.get("message").and_then(Value::as_str))
                .unwrap_or("solc reported an error without a message")
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
    use super::{compile_solidity_source, standard_json_input};

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
        let input = standard_json_input(FALLBACK_SOURCE, "osaka");

        assert_eq!(input["language"], "Solidity");
        assert_eq!(input["sources"]["main.sol"]["content"], FALLBACK_SOURCE);
        assert_eq!(input["settings"]["evmVersion"], "osaka");
        assert_eq!(input["settings"]["metadata"]["appendCBOR"], false);
        assert_eq!(input["settings"]["metadata"]["bytecodeHash"], "none");
    }

    #[test]
    #[ignore = "requires RAPPIE_SOL_SOLC, SOLC_PATH, or solc on PATH"]
    fn compiles_minimal_fallback_contract() {
        let bytecode =
            compile_solidity_source(FALLBACK_SOURCE).expect("fallback source should compile");

        assert!(!bytecode.is_empty());
    }
}
