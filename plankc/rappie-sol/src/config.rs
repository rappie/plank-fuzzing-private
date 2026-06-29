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
const DEFAULT_BACKENDS: [&str; 2] = ["sir-debug", "sir-release-csudl"];

static CONFIGURED_BACKENDS: OnceLock<Result<OracleBackendSet, BackendConfigError>> =
    OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleBackendSpec {
    Solidity(SolidityBackendSpec),
    Plank(BackendSpec),
}

impl OracleBackendSpec {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Solidity(backend) => backend.name,
            Self::Plank(backend) => backend.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleBackendSet {
    pub backends: Vec<OracleBackendSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendConfigError {
    Read { path: PathBuf, message: String },
    Parse { path: Option<PathBuf>, message: String },
    UnknownBackend { name: String },
    TooFewBackends { enabled: usize },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackendConfigToml {
    #[serde(default)]
    backends: BTreeMap<String, bool>,
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
        let backends = Self::load_from_optional_path(&path, !explicit)?;
        eprintln!(
            "rappie-sol testing {} backends: {}",
            backends.backend_count(),
            backends.backend_names_csv()
        );
        Ok(backends)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, BackendConfigError> {
        Self::load_from_optional_path(path.as_ref(), false)
    }

    pub fn from_toml_str(contents: &str) -> Result<Self, BackendConfigError> {
        Self::from_toml_str_with_path(contents, None)
    }

    pub fn backend_names_csv(&self) -> String {
        self.backend_names().join(", ")
    }

    fn backend_count(&self) -> usize {
        self.backends.len()
    }

    fn backend_names(&self) -> Vec<&'static str> {
        self.backends.iter().map(OracleBackendSpec::name).collect()
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
        for name in config.backends.keys() {
            if find_oracle_backend(name).is_none() {
                return Err(BackendConfigError::UnknownBackend { name: name.clone() });
            }
        }

        let backends = known_oracle_backends()
            .filter(|backend| config.backends.get(backend.name()).copied().unwrap_or(false))
            .collect::<Vec<_>>();

        validate_backend_count(backends.len())?;

        Ok(Self { backends })
    }
}

impl Default for OracleBackendSet {
    fn default() -> Self {
        let backends = DEFAULT_BACKENDS
            .into_iter()
            .map(|name| find_oracle_backend(name).expect("default backend must be registered"))
            .collect::<Vec<_>>();

        Self { backends }
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
            Self::UnknownBackend { name } => {
                write!(f, "unknown backend in backend config: {name}")
            }
            Self::TooFewBackends { enabled } => {
                write!(f, "backend config must enable at least two backends, got {enabled}")
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

pub(crate) fn validate_backend_count(enabled: usize) -> Result<(), BackendConfigError> {
    if enabled < 2 {
        return Err(BackendConfigError::TooFewBackends { enabled });
    }

    Ok(())
}

pub(crate) fn known_oracle_backends() -> impl Iterator<Item = OracleBackendSpec> {
    known_solidity_backends()
        .map(OracleBackendSpec::Solidity)
        .chain(DEFAULT_PLANK_BACKENDS.into_iter().map(OracleBackendSpec::Plank))
}

fn known_solidity_backends() -> impl Iterator<Item = SolidityBackendSpec> {
    std::iter::once(SOLIDITY_REFERENCE_BACKEND).chain(DEFAULT_SOLIDITY_BACKENDS)
}

fn find_oracle_backend(name: &str) -> Option<OracleBackendSpec> {
    known_oracle_backends().find(|backend| backend.name() == name)
}

#[cfg(test)]
mod tests {
    use super::{
        BackendConfigError, OracleBackendSet, OracleBackendSpec, find_oracle_backend,
        known_oracle_backends,
    };
    use std::env;

    fn backend_names(backends: &OracleBackendSet) -> Vec<&'static str> {
        backends.backends.iter().map(OracleBackendSpec::name).collect()
    }

    #[test]
    fn default_backend_set_uses_focused_sir_pair() {
        let backends = OracleBackendSet::default();

        assert_eq!(backend_names(&backends), vec!["sir-debug", "sir-release-csudl"]);
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
    fn tracked_backend_config_uses_focused_sir_pair() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(super::DEFAULT_CONFIG_FILE);
        let backends =
            OracleBackendSet::load_from_path(path).expect("tracked backend config should parse");

        assert_eq!(backend_names(&backends), vec!["sir-debug", "sir-release-csudl"]);
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
    fn config_enables_backends_in_registry_order() {
        let backends = OracleBackendSet::from_toml_str(
            r#"
[backends]
sir-release-csudl = true
solx-reference = true
sir-debug = true
"#,
        )
        .expect("config should parse");

        assert_eq!(
            backend_names(&backends),
            vec!["solx-reference", "sir-debug", "sir-release-csudl"]
        );
    }

    #[test]
    fn config_can_enable_solidity_only_pair() {
        let backends = OracleBackendSet::from_toml_str(
            r#"
[backends]
solc-noopt-legacy = true
solc-opt-legacy = true
"#,
        )
        .expect("config should parse");

        assert_eq!(backend_names(&backends), vec!["solc-noopt-legacy", "solc-opt-legacy"]);
    }

    #[test]
    fn unknown_backend_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str(
                r#"
[backends]
sir-release-wow = true
"#
            ),
            Err(BackendConfigError::UnknownBackend { name })
                if name == "sir-release-wow"
        ));
    }

    #[test]
    fn false_unknown_backend_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str(
                r#"
[backends]
sir-debug = true
sir-release-csudl = true
sir-release-wow = false
"#
            ),
            Err(BackendConfigError::UnknownBackend { name })
                if name == "sir-release-wow"
        ));
    }

    #[test]
    fn missing_backend_table_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str(""),
            Err(BackendConfigError::TooFewBackends { enabled: 0 })
        ));
    }

    #[test]
    fn empty_backend_table_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str("[backends]\n"),
            Err(BackendConfigError::TooFewBackends { enabled: 0 })
        ));
    }

    #[test]
    fn one_enabled_backend_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str(
                r#"
[backends]
sir-debug = true
sir-release-csudl = false
"#
            ),
            Err(BackendConfigError::TooFewBackends { enabled: 1 })
        ));
    }

    #[test]
    fn old_reference_field_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str(r#"reference = "solx-reference""#),
            Err(BackendConfigError::Parse { .. })
        ));
    }

    #[test]
    fn old_solidity_table_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str(
                r#"
[solidity]
solx-reference = true
"#
            ),
            Err(BackendConfigError::Parse { .. })
        ));
    }

    #[test]
    fn old_plank_table_is_rejected() {
        assert!(matches!(
            OracleBackendSet::from_toml_str(
                r#"
[plank]
sir-debug = true
"#
            ),
            Err(BackendConfigError::Parse { .. })
        ));
    }

    #[test]
    fn backend_names_csv_lists_enabled_backends() {
        let backends = OracleBackendSet::from_toml_str(
            r#"
[backends]
sir-debug = true
sir-release-csudl = true
"#,
        )
        .expect("config should parse");

        assert_eq!(backends.backend_names_csv(), "sir-debug, sir-release-csudl");
    }

    #[test]
    fn known_oracle_backends_are_unique() {
        let mut names = std::collections::BTreeSet::new();

        for backend in known_oracle_backends() {
            assert!(names.insert(backend.name()), "duplicate backend name {}", backend.name());
        }
    }

    #[test]
    fn default_backends_are_registered() {
        assert!(matches!(find_oracle_backend("sir-debug"), Some(OracleBackendSpec::Plank(_))));
        assert!(matches!(
            find_oracle_backend("sir-release-csudl"),
            Some(OracleBackendSpec::Plank(_))
        ));
    }
}
