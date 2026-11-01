//! Search root resolution and validation.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    Dir,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRoot {
    pub path: PathBuf,
    pub kind: RootKind,
    local: bool,
}

#[derive(Debug)]
pub enum ScopeError {
    NoHomeFolders,
    Inaccessible { path: PathBuf, source: io::Error },
}

fn inaccessible(path: &Path) -> impl FnOnce(io::Error) -> ScopeError + '_ {
    move |source| ScopeError::Inaccessible {
        path: path.to_path_buf(),
        source,
    }
}

impl SearchRoot {
    /// The current working directory, walked as `.`.
    pub fn local() -> Result<Self, ScopeError> {
        let dot = Path::new(".");
        let cwd = std::env::current_dir().map_err(inaccessible(dot))?;
        fs::read_dir(&cwd).map_err(inaccessible(&cwd))?;
        Ok(Self {
            path: dot.to_path_buf(),
            kind: RootKind::Dir,
            local: true,
        })
    }

    /// The path as it should be printed: local results lose their leading `./`.
    pub fn display<'a>(&self, entry: &'a Path) -> &'a Path {
        if self.local {
            entry.strip_prefix(".").unwrap_or(entry)
        } else {
            entry
        }
    }
}

/// An explicit `--path` root: a directory, or a single file evaluated directly.
/// A symlinked root resolves to its target.
pub fn resolve_explicit(path: &Path) -> Result<SearchRoot, ScopeError> {
    let meta = fs::metadata(path).map_err(inaccessible(path))?;
    let kind = if meta.is_dir() {
        fs::read_dir(path).map_err(inaccessible(path))?;
        RootKind::Dir
    } else {
        RootKind::File
    };
    Ok(SearchRoot {
        path: path.to_path_buf(),
        kind,
        local: false,
    })
}

/// User folders for `--home`. `candidates` come from the platform; any that is
/// missing, is `home` itself, or sits inside another candidate is dropped, so home itself is never searched.
pub fn resolve_home(
    candidates: &[PathBuf],
    home: Option<&Path>,
) -> Result<Vec<SearchRoot>, ScopeError> {
    let home_id = home.and_then(|h| fs::canonicalize(h).ok());
    let picked: Vec<(PathBuf, &PathBuf)> = candidates
        .iter()
        .filter_map(|c| fs::canonicalize(c).ok().map(|id| (id, c)))
        .filter(|(id, _)| id.is_dir() && Some(id) != home_id.as_ref())
        .collect();

    // Drop repeats and folders nested in another pick, so no entry is reported twice.
    let roots: Vec<SearchRoot> = picked
        .iter()
        .enumerate()
        .filter(|&(i, (id, _))| {
            !picked
                .iter()
                .enumerate()
                .any(|(j, (other, _))| j != i && id.starts_with(other) && (id != other || j < i))
        })
        .map(|(_, (id, given))| SearchRoot {
            path: if given.is_absolute() {
                (*given).clone()
            } else {
                id.clone()
            },
            kind: RootKind::Dir,
            local: false,
        })
        .collect();

    if roots.is_empty() {
        Err(ScopeError::NoHomeFolders)
    } else {
        Ok(roots)
    }
}
