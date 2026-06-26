use std::{
    fmt::{self, Write},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StdMode {
    None,
    RepoStd,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlankSourceFile {
    pub path: PathBuf,
    pub source: String,
}

impl PlankSourceFile {
    pub fn new(path: impl Into<PathBuf>, source: String) -> Self {
        Self { path: path.into(), source }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlankSourceSet {
    pub entry_path: PathBuf,
    pub files: Vec<PlankSourceFile>,
    pub std_mode: StdMode,
}

impl PlankSourceSet {
    pub fn single_main(source: String) -> Self {
        Self {
            entry_path: PathBuf::from("main.plk"),
            files: vec![PlankSourceFile::new("main.plk", source)],
            std_mode: StdMode::None,
        }
    }

    pub fn main_source(&self) -> Option<&str> {
        self.files.iter().find(|file| file.path == self.entry_path).map(|file| file.source.as_str())
    }
}

impl fmt::Display for PlankSourceSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, file) in self.files.iter().enumerate() {
            if index > 0 {
                f.write_char('\n')?;
            }
            writeln!(f, "== {} ==", file.path.display())?;
            f.write_str(&file.source)?;
            if !file.source.ends_with('\n') {
                f.write_char('\n')?;
            }
        }
        Ok(())
    }
}
