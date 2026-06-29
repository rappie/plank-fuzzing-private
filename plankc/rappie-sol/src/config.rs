use crate::{
    DEFAULT_SOLIDITY_BACKENDS, SOLIDITY_REFERENCE_BACKEND, SolidityBackendSpec,
    oracle::{BackendSpec, DEFAULT_PLANK_BACKENDS},
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env, fmt, fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const CONFIG_ENV_VAR: &str = "RAPPIE_SOL_BACKENDS_CONFIG";
const DEFAULT_CONFIG_FILE: &str = "backends.toml";

static CONFIGURED_BACKENDS: OnceLock<Result<OracleBackendSet, BackendConfigError>> =
    OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleBackendSet {
    pub reference: SolidityBackendSpec,
    pub solidity_peers: Vec<SolidityBackendSpec>,
    pub plank_backends: Vec<BackendSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendConfigError {
    Read { path: PathBuf, message: String },
    Parse { path: Option<PathBuf>, message: String },
    UnknownReference { name: String },
    UnknownSolidityBackend { name: String },
    UnknownPlankBackend { name: String },
    EmptyBackendSet,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackendConfigToml {
    #[serde(default)]
    reference: Option<String>,
    #[serde(default)]
    solidity: BTreeMap<String, bool>,
    #[serde(default)]
    plank: BTreeMap<String, bool>,
}

impl OracleBackendSet {
    pub fn configured() -> Result<&'static Self, BackendConfigError> {
        match CONFIGURED_BACKENDS.get_or_init(Self::load_configured) {
            Ok(backends) => Ok(backends),
            Err(err) => Err(err.clone()),
        }
    }

    pub fn load_configured() -> Result<Self, BackendConfigError> {
        let (path, explicit) = configured_path();
        Self::load_from_optional_path(&path, !explicit)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, BackendConfigError> {
        Self::load_from_optional_path(path.as_ref(), false)
    }

    pub fn from_toml_str(contents: &str) -> Result<Self, BackendConfigError> {
        Self::from_toml_str_with_path(contents, None)
    }

    fn load_from_optional_path(
        path: &Path,
        default_if_missing: bool,
    ) -> Result<Self, BackendConfigError> {
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(err) if default_if_missing && err.kind() == io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(err) => {
                return Err(BackendConfigError::Read {
                    path: path.to_path_buf(),
                    message: err.to_string(),
                });
            }
        };

        Self::from_toml_str_with_path(&contents, Some(path.to_path_buf()))
    }

    fn from_toml_str_with_path(
        contents: &str,
        path: Option<PathBuf>,
    ) -> Result<Self, BackendConfigError> {
        let config = toml::from_str::<BackendConfigToml>(contents).map_err(|err| {
            BackendConfigError::Parse { path: path.clone(), message: err.to_string() }
        })?;

        Self::from_config(config)
    }

    fn from_config(config: BackendConfigToml) -> Result<Self, BackendConfigError> {
        for name in config.solidity.keys() {
            if find_solidity_backend(name).is_none() {
                return Err(BackendConfigError::UnknownSolidityBackend { name: name.clone() });
            }
        }

        for name in config.plank.keys() {
            if find_plank_backend(name).is_none() {
                return Err(BackendConfigError::UnknownPlankBackend { name: name.clone() });
            }
        }

        let reference = match config.reference.as_deref() {
            Some(name) => find_solidity_backend(name)
                .ok_or_else(|| BackendConfigError::UnknownReference { name: name.to_string() })?,
            None => SOLIDITY_REFERENCE_BACKEND,
        };

        let solidity_peers = known_solidity_backends()
            .filter(|backend| backend.name != reference.name)
            .filter(|backend| {
                config
                    .solidity
                    .get(backend.name)
                    .copied()
                    .unwrap_or_else(|| default_solidity_peer_enabled(*backend))
            })
            .collect::<Vec<_>>();

        let plank_backends = DEFAULT_PLANK_BACKENDS
            .into_iter()
            .filter(|backend| config.plank.get(backend.name).copied().unwrap_or(true))
            .collect::<Vec<_>>();

        if solidity_peers.is_empty() && plank_backends.is_empty() {
            return Err(BackendConfigError::EmptyBackendSet);
        }

        Ok(Self { reference, solidity_peers, plank_backends })
    }
}

impl Default for OracleBackendSet {
    fn default() -> Self {
        Self {
            reference: SOLIDITY_REFERENCE_BACKEND,
            solidity_peers: DEFAULT_SOLIDITY_BACKENDS.to_vec(),
            plank_backends: DEFAULT_PLANK_BACKENDS.to_vec(),
        }
    }
}

impl fmt::Display for BackendConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, message } => {
                write!(f, "could not read backend config at {}: {message}", path.display())
            }
            Self::Parse { path: Some(path), message } => {
                write!(f, "could not parse backend config at {}: {message}", path.display())
            }
            Self::Parse { path: None, message } => {
                write!(f, "could not parse backend config: {message}")
            }
            Self::UnknownReference { name } => {
                write!(f, "unknown Solidity reference backend in backend config: {name}")
            }
            Self::UnknownSolidityBackend { name } => {
                write!(f, "unknown Solidity backend in backend config: {name}")
            }
            Self::UnknownPlankBackend { name } => {
                write!(f, "unknown Plank backend in backend config: {name}")
            }
            Self::EmptyBackendSet => {
                f.write_str("backend config must enable at least one candidate backend")
            }
        }
    }
}

impl std::error::Error for BackendConfigError {}

fn configured_path() -> (PathBuf, bool) {
    if let Some(path) = env::var_os(CONFIG_ENV_VAR) {
        return (PathBuf::from(path), true);
    }

    (PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_CONFIG_FILE), false)
}

fn known_solidity_backends() -> impl Iterator<Item = SolidityBackendSpec> {
    std::iter::once(SOLIDITY_REFERENCE_BACKEND).chain(DEFAULT_SOLIDITY_BACKENDS)
}

fn find_solidity_backend(name: &str) -> Option<SolidityBackendSpec> {
    known_solidity_backends().find(|backend| backend.name == name)
}

fn find_plank_backend(name: &str) -> Option<BackendSpec> {
    DEFAULT_PLANK_BACKENDS.into_iter().find(|backend| backend.name == name)
}

fn default_solidity_peer_enabled(backend: SolidityBackendSpec) -> bool {
    DEFAULT_SOLIDITY_BACKENDS.iter().any(|default_backend| default_backend.name == backend.name)
}

#[cfg(test)]
mod tests {
    use super::{BackendConfigError, OracleBackendSet};
    use crate::{DEFAULT_PLANK_BACKENDS, DEFAULT_SOLIDITY_BACKENDS, SOLIDITY_REFERENCE_BACKEND};
    use std::{env, fmt::Write};

    #[test]
    fn default_backend_set_matches_existing_registry() {
        let backends = OracleBackendSet::default();

        assert_eq!(backends.reference, SOLIDITY_REFERENCE_BACKEND);
        assert_eq!(backends.solidity_peers, DEFAULT_SOLIDITY_BACKENDS);
        assert_eq!(backends.plank_backends, DEFAULT_PLANK_BACKENDS);
    }

    #[test]
    fn missing_default_config_uses_defaults() {
        let path = env::temp_dir()
            .join(format!("rappie-sol-missing-backends-{}.toml", std::process::id()));

        let backends = OracleBackendSet::load_from_optional_path(&path, true)
            .expect("missing default config should use defaults");

        assert_eq!(backends, OracleBackendSet::default());
    }

    #[test]
    fn explicit_missing_config_is_an_error() {
        let path = env::temp_dir()
            .join(format!("rappie-sol-explicit-missing-backends-{}.toml", std::process::id()));

        assert!(matches!(
            OracleBackendSet::load_from_path(path),
            Err(BackendConfigError::Read { .. })
        ));
    }

    #[test]
    fn config_can_disable_one_plank_backend() {
        let backends = OracleBackendSet::from_toml_str(
            r#"
[plank]
sona-o2 = false
"#,
        )
        .expect("config should parse");

        assert_eq!(backends.plank_backends.len(), DEFAULT_PLANK_BACKENDS.len() - 1);
        assert!(!backends.plank_backends.iter().any(|backend| backend.name == "sona-o2"));
    }

    #[test]
    fn config_can_disable_one_solidity_peer() {
        let backends = OracleBackendSet::from_toml_str(
            r#"
[solidity]
solc-opt-via-ir = false
"#,
        )
        .expect("config should parse");

        assert_eq!(backends.solidity_peers.len(), DEFAULT_SOLIDITY_BACKENDS.len() - 1);
        assert!(!backends.solidity_peers.iter().any(|backend| backend.name == "solc-opt-via-ir"));
    }

    #[test]
    fn config_can_select_solidity_reference_backend() {
        let backends = OracleBackendSet::from_toml_str(
            r#"
reference = "solc-noopt-via-ir"

[solidity]
solx-reference = true
"#,
        )
        .expect("config should parse");

        assert_eq!(backends.reference.name, "solc-noopt-via-ir");
        assert!(backends.solidity_peers.iter().any(|backend| backend.name == "solx-reference"));
        assert!(!backends.solidity_peers.iter().any(|backend| backend.name == "solc-noopt-via-ir"));
    }

    #[test]
    fn unknown_solidity_backend_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str(
                r#"
[solidity]
solc-but-faster = true
"#
            ),
            Err(BackendConfigError::UnknownSolidityBackend { name })
                if name == "solc-but-faster"
        ));
    }

    #[test]
    fn unknown_plank_backend_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str(
                r#"
[plank]
sir-release-wow = true
"#
            ),
            Err(BackendConfigError::UnknownPlankBackend { name })
                if name == "sir-release-wow"
        ));
    }

    #[test]
    fn unknown_reference_backend_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str(r#"reference = "solc-mystery""#),
            Err(BackendConfigError::UnknownReference { name }) if name == "solc-mystery"
        ));
    }

    #[test]
    fn empty_candidate_set_is_rejected() {
        let mut config = String::from("[solidity]\n");
        config.push_str("solc-noopt-legacy = false\n");
        config.push_str("solc-noopt-via-ir = false\n");
        config.push_str("solc-opt-legacy = false\n");
        config.push_str("solc-opt-via-ir = false\n");
        config.push_str("\n[plank]\n");
        for backend in DEFAULT_PLANK_BACKENDS {
            writeln!(config, "{} = false", backend.name).expect("string write should not fail");
        }

        assert!(matches!(
            OracleBackendSet::from_toml_str(&config),
            Err(BackendConfigError::EmptyBackendSet)
        ));
    }

    #[test]
    fn filtered_backends_keep_registry_order() {
        let backends = OracleBackendSet::from_toml_str(
            r#"
[plank]
sir-release = false
sir-release-s = false
"#,
        )
        .expect("config should parse");

        let actual =
            backends.plank_backends.iter().take(4).map(|backend| backend.name).collect::<Vec<_>>();

        assert_eq!(actual, vec!["sir-debug", "sir-release-c", "sir-release-u", "sir-release-d"]);
    }
}
