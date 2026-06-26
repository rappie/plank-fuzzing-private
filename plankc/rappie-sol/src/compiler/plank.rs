use crate::sources::{PlankSourceSet, StdMode};
use plank_driver::{BackendKind, Driver};
use plank_evm::EvmVersion;
use plank_source::source_fs::InMemoryFs;
use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

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

pub(crate) fn compile_plank_sources(
    sources: &PlankSourceSet,
    backend: BackendKind,
    optimizations: Option<&str>,
) -> Result<Vec<u8>, CompileError> {
    let mut fs = InMemoryFs::new();
    for file in &sources.files {
        fs.add_file(&file.path, file.source.clone());
    }

    if sources.std_mode == StdMode::RepoStd {
        load_repo_std(&mut fs).map_err(|err| {
            CompileError::new(format!("failed to load repo std into in-memory fs: {err}"))
        })?;
    }

    let mut driver = Driver::new(&fs);
    driver.register_module("gen", PathBuf::from("gen"));
    if sources.std_mode == StdMode::RepoStd {
        driver.register_std(PathBuf::from("std"));
    }

    let project = driver
        .load_project(&sources.entry_path)
        .ok_or_else(|| CompileError::new(render_diagnostics(&driver)))?;

    let hir = driver.lower_hir(&project);
    let mir = driver.evaluate_hir(&hir, project.core_ops_source, EvmVersion::Osaka);
    if driver.session.has_errors() {
        return Err(CompileError::new(render_diagnostics(&driver)));
    }

    driver
        .emit_bytecode_with_backend(&mir, optimizations, false, false, false, backend)
        .map_err(CompileError::new)
}

fn load_repo_std(fs: &mut InMemoryFs) -> io::Result<()> {
    let std_dir = repo_std_dir();
    load_dir(fs, &std_dir, Path::new("std"))
}

fn repo_std_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("rappie-sol is under plankc/")
        .join("std")
}

fn load_dir(fs: &mut InMemoryFs, real_dir: &Path, fs_prefix: &Path) -> io::Result<()> {
    for entry in fs::read_dir(real_dir)? {
        let entry = entry?;
        let real_path = entry.path();
        let fs_path = fs_prefix.join(entry.file_name());
        if real_path.is_dir() {
            load_dir(fs, &real_path, &fs_path)?;
        } else {
            fs.add_file(fs_path, fs::read_to_string(real_path)?);
        }
    }

    Ok(())
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
