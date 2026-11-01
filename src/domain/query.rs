//! Immutable query compilation and name/path evaluation.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    Name,
    Extension,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    Empty,
    EmptyExtension,
    Absolute,
    ParentComponent,
    NoComponents,
}

/// A query whose mode is fixed at compile time and never revised by results.
#[derive(Debug, Clone)]
pub struct CompiledQuery {
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Kind {
    Name(Needle),
    Extension(Needle),
    Path(PathQuery),
}

impl CompiledQuery {
    pub fn compile(raw: &OsStr) -> Result<Self, QueryError> {
        let bytes = raw.as_encoded_bytes();
        let kind = match bytes {
            [] => return Err(QueryError::Empty),
            _ if bytes.iter().copied().any(is_separator) => Kind::Path(PathQuery::parse(bytes)?),
            [b'.'] => return Err(QueryError::EmptyExtension),
            [b'.', b'.'] => return Err(QueryError::ParentComponent),
            [b'.', ext @ ..] if !ext.contains(&b'.') => Kind::Extension(Needle::new(ext)),
            _ => Kind::Name(Needle::new(bytes)),
        };
        Ok(Self { kind })
    }

    pub fn mode(&self) -> QueryMode {
        match self.kind {
            Kind::Name(_) => QueryMode::Name,
            Kind::Extension(_) => QueryMode::Extension,
            Kind::Path(_) => QueryMode::Path,
        }
    }

    /// `rel` is the entry path relative to its search root.
    pub fn matches(&self, rel: &Path, is_dir: bool) -> bool {
        match &self.kind {
            Kind::Name(n) => rel.file_name().is_some_and(|f| n.within(f)),
            Kind::Extension(n) => rel.file_name().is_some_and(|f| n.is_extension_of(f)),
            Kind::Path(p) => p.matches(rel, is_dir),
        }
    }

    /// Whether `matches` reads `is_dir` (a `dir/` query).
    pub fn needs_kind(&self) -> bool {
        matches!(&self.kind, Kind::Path(p) if p.dir_only)
    }

    /// True when a hidden component literally named in a path query names this entry, exempting it from the hidden rule.
    pub fn names_hidden(&self, rel: &Path) -> bool {
        match &self.kind {
            Kind::Path(p) => (0..p.components.len())
                .any(|i| p.components[i].hidden && p.tail_matches(rel, i + 1)),
            _ => false,
        }
    }
}

fn is_separator(b: u8) -> bool {
    b == b'/' || (cfg!(windows) && b == b'\\')
}

#[derive(Debug, Clone)]
struct PathQuery {
    components: Vec<Needle>,
    anchored: bool,
    dir_only: bool,
}

impl PathQuery {
    fn parse(bytes: &[u8]) -> Result<Self, QueryError> {
        if is_separator(bytes[0]) {
            return Err(QueryError::Absolute);
        }
        let mut components = Vec::new();
        for part in bytes.split(|&b| is_separator(b)) {
            match part {
                b"" | b"." => {}
                b".." => return Err(QueryError::ParentComponent),
                _ => components.push(Needle::new(part)),
            }
        }
        if components.is_empty() {
            return Err(QueryError::NoComponents);
        }
        Ok(Self {
            components,
            anchored: bytes[0] == b'.' && bytes.get(1).copied().is_some_and(is_separator),
            // `src/` and `src/.` both name the directory `src`.
            dir_only: matches!(
                bytes.split(|&b| is_separator(b)).next_back(),
                Some(b"" | b".")
            ),
        })
    }

    fn matches(&self, rel: &Path, is_dir: bool) -> bool {
        (is_dir || !self.dir_only) && self.tail_matches(rel, self.components.len())
    }

    /// Whether `rel` ends with the first `len` query components (anchored: consists of exactly
    /// them). Intermediate components need equality; the query's last component is a prefix of
    /// the entry name, or equal to it for a `dir/` query.
    fn tail_matches(&self, rel: &Path, len: usize) -> bool {
        let last = self.components.len() - 1;
        let mut names = rel.components().rev().filter_map(|c| match c {
            Component::Normal(n) => Some(n),
            _ => None,
        });
        for (i, needle) in self.components[..len].iter().enumerate().rev() {
            let Some(name) = names.next() else {
                return false;
            };
            let ok = if i == last && !self.dir_only {
                needle.prefix_of(name)
            } else {
                needle.eq(name)
            };
            if !ok {
                return false;
            }
        }
        !self.anchored || names.next().is_none()
    }
}

/// Case-folded query text, compared with case-folded entry names as bytes.
#[derive(Debug, Clone)]
struct Needle {
    folded: Vec<u8>,
    hidden: bool,
}

impl Needle {
    fn new(raw: &[u8]) -> Self {
        Self {
            folded: fold(raw).into_owned(),
            hidden: raw.first() == Some(&b'.'),
        }
    }

    fn eq(&self, name: &OsStr) -> bool {
        *fold(name.as_encoded_bytes()) == *self.folded
    }

    fn prefix_of(&self, name: &OsStr) -> bool {
        fold(name.as_encoded_bytes()).starts_with(&self.folded)
    }

    fn within(&self, name: &OsStr) -> bool {
        let hay = fold(name.as_encoded_bytes());
        hay.windows(self.folded.len()).any(|w| w == self.folded)
    }

    /// Exact final extension: text after the last dot, where a leading dot starts no extension.
    fn is_extension_of(&self, name: &OsStr) -> bool {
        let hay = fold(name.as_encoded_bytes());
        match hay.iter().rposition(|&b| b == b'.') {
            Some(i) if i > 0 => hay[i + 1..] == *self.folded,
            _ => false,
        }
    }
}

/// Unicode lowercase for UTF-8 text; ASCII-only folding otherwise, so non-UTF-8 names are never
/// decoded lossily. UTF-8 is self-synchronizing, so byte comparisons stay exact.
fn fold(bytes: &[u8]) -> Cow<'_, [u8]> {
    if bytes.is_ascii() {
        if bytes.iter().any(u8::is_ascii_uppercase) {
            Cow::Owned(bytes.to_ascii_lowercase())
        } else {
            Cow::Borrowed(bytes)
        }
    } else if let Ok(s) = std::str::from_utf8(bytes) {
        Cow::Owned(s.to_lowercase().into_bytes())
    } else {
        Cow::Owned(bytes.to_ascii_lowercase())
    }
}
