use plank_driver::{BackendKind, Driver};
use plank_evm::EvmVersion;
use plank_source::source_fs::InMemoryFs;
use std::{fmt, path::Path};

const MAIN_PATH: &str = "main.plk";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompileError {
    diagnostics: String,
}

impl CompileError {
    fn new(diagnostics: String) -> Self {
        Self { diagnostics }
    }

    pub(crate) fn diagnostics(&self) -> &str {
        &self.diagnostics
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.diagnostics)
    }
}

impl std::error::Error for CompileError {}

pub(crate) fn compile_plank_source(
    source: &str,
    backend: BackendKind,
) -> Result<Vec<u8>, CompileError> {
    let mut fs = InMemoryFs::new();
    fs.add_file(MAIN_PATH, source.to_string());

    let mut driver = Driver::new(&fs);
    let project = driver
        .load_project(Path::new(MAIN_PATH))
        .ok_or_else(|| CompileError::new(render_diagnostics(&driver)))?;

    let hir = driver.lower_hir(&project);
    let mir = driver.evaluate_hir(&hir, project.core_ops_source, EvmVersion::Osaka);
    if driver.session.has_errors() {
        return Err(CompileError::new(render_diagnostics(&driver)));
    }

    driver
        .emit_bytecode_with_backend(&mir, None, false, false, false, backend)
        .map_err(CompileError::new)
}

fn render_diagnostics<F: plank_source::SourceFs>(driver: &Driver<'_, F>) -> String {
    if driver.session.diagnostics().is_empty() {
        return "compilation failed without diagnostics".to_string();
    }

    driver
        .session
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.render_plain(&driver.session))
        .collect::<Vec<_>>()
        .join("\n----\n")
}
