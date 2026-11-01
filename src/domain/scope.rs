//! Search root resolution and validation.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

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
    NoHome,
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

    /// The path as it should be printed: local results lose their leading `./`, unless the
    /// rest starts with `-` and would read as an option to the next command.
    pub fn display<'a>(&self, entry: &'a Path) -> &'a Path {
        if !self.local {
            return entry;
        }
        match entry.strip_prefix(".") {
            Ok(rel) if !rel.as_os_str().as_encoded_bytes().starts_with(b"-") => rel,
            _ => entry,
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

/// The home directory, for `--home`.
pub fn resolve_home(home: Option<&Path>) -> Result<SearchRoot, ScopeError> {
    resolve_explicit(home.ok_or(ScopeError::NoHome)?)
}

/// A query argument that names a place rather than a name: absolute, `~`-based, through `..`,
/// or `.` itself. Relative paths like `src/main` stay path queries.
pub fn is_location(arg: &OsStr) -> bool {
    let b = arg.as_encoded_bytes();
    is_here(b)
        || b == b"~"
        || b.strip_prefix(b"~")
            .is_some_and(|rest| rest.first().copied().is_some_and(is_sep))
        || Path::new(arg).components().any(|c| {
            matches!(
                c,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
}

fn is_here(b: &[u8]) -> bool {
    matches!(b, [b'.'] | [b'.', b'/']) || (cfg!(windows) && b == b".\\")
}

fn is_sep(b: u8) -> bool {
    b == b'/' || (cfg!(windows) && b == b'\\')
}

/// A location split into where to search and, when its last component does not exist, the
/// name to search for there: `~/Downloads/report` is `report` in `~/Downloads`.
#[derive(Debug)]
pub struct Location {
    pub root: SearchRoot,
    pub term: Option<OsString>,
}

pub fn resolve_location(arg: &OsStr, home: Option<&Path>) -> Result<Location, ScopeError> {
    let b = arg.as_encoded_bytes();
    if is_here(b) {
        return Ok(Location {
            root: SearchRoot::local()?,
            term: None,
        });
    }
    let path = expand_tilde(arg, home)?;
    let source = match fs::metadata(&path) {
        Ok(_) => {
            return Ok(Location {
                root: resolve_explicit(&path)?,
                term: None,
            });
        }
        Err(e) => e,
    };
    let trailing_sep = b.last().copied().is_some_and(is_sep);
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if !trailing_sep && parent.is_dir() => Ok(Location {
            root: resolve_explicit(parent)?,
            term: Some(name.to_owned()),
        }),
        _ => Err(ScopeError::Inaccessible { path, source }),
    }
}

fn expand_tilde(arg: &OsStr, home: Option<&Path>) -> Result<PathBuf, ScopeError> {
    let b = arg.as_encoded_bytes();
    if b == b"~" {
        return Ok(home.ok_or(ScopeError::NoHome)?.to_path_buf());
    }
    match Path::new(arg).strip_prefix("~") {
        Ok(rest) if b.get(1).copied().is_some_and(is_sep) => {
            Ok(home.ok_or(ScopeError::NoHome)?.join(rest))
        }
        _ => Ok(PathBuf::from(arg)),
    }
}

/// Drops roots that repeat or sit inside another root, so no entry is reported twice.
/// Order is kept.
pub fn dedupe(roots: Vec<SearchRoot>) -> Vec<SearchRoot> {
    let ids: Vec<PathBuf> = roots
        .iter()
        .map(|r| fs::canonicalize(&r.path).unwrap_or_else(|_| r.path.clone()))
        .collect();
    roots
        .into_iter()
        .enumerate()
        .filter(|&(i, _)| {
            !ids.iter().enumerate().any(|(j, other)| {
                j != i && ids[i].starts_with(other) && (ids[i] != *other || j < i)
            })
        })
        .map(|(_, r)| r)
        .collect()
}
