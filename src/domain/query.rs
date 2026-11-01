//! Immutable query compilation and name/path evaluation.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Component, Path};

use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    Name,
    Extension,
    Glob,
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

/// How directly a query names an entry. A named entry is exempt from the hidden rule and the
/// generated-directory list: `Leaf` may be printed but is not entered, `Reach` is also entered
/// because a path term continues below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Named {
    No,
    Leaf,
    Reach,
}

/// All terms of one search. An entry matches when every term does; no terms match everything.
#[derive(Debug, Clone)]
pub struct Query {
    terms: Vec<CompiledQuery>,
}

impl Query {
    pub fn new(terms: Vec<CompiledQuery>) -> Self {
        Self { terms }
    }

    pub fn matches(&self, rel: &Path, is_dir: bool) -> bool {
        self.terms.iter().all(|t| t.matches(rel, is_dir))
    }

    pub fn needs_kind(&self) -> bool {
        self.terms.iter().any(CompiledQuery::needs_kind)
    }

    pub fn names(&self, rel: &Path) -> Named {
        self.terms
            .iter()
            .map(|t| t.names(rel))
            .max()
            .unwrap_or(Named::No)
    }
}

/// One term whose mode is fixed at compile time and never revised by results.
#[derive(Debug, Clone)]
pub struct CompiledQuery {
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Kind {
    /// Substring of the name; a dotted term must start at a word boundary.
    Name {
        needle: Needle,
        boundary: bool,
    },
    /// `.ext` or `.tar.gz`, stored with its dot: a name ending in it, or exactly it.
    Extension(Needle),
    Glob(Pattern),
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
            _ if is_glob(bytes) => Kind::Glob(Pattern::new(bytes)),
            [b'.', ..] => Kind::Extension(Needle::new(bytes)),
            _ => Kind::Name {
                needle: Needle::new(bytes),
                boundary: bytes.contains(&b'.'),
            },
        };
        Ok(Self { kind })
    }

    pub fn mode(&self) -> QueryMode {
        match self.kind {
            Kind::Name { .. } => QueryMode::Name,
            Kind::Extension(_) => QueryMode::Extension,
            Kind::Glob(_) => QueryMode::Glob,
            Kind::Path(_) => QueryMode::Path,
        }
    }

    /// `rel` is the entry path relative to its search root.
    pub fn matches(&self, rel: &Path, is_dir: bool) -> bool {
        if let Kind::Path(p) = &self.kind {
            return p.matches(rel, is_dir);
        }
        let Some(name) = rel.file_name() else {
            return false;
        };
        let name = fold(name.as_encoded_bytes());
        match &self.kind {
            Kind::Name { needle, boundary } => needle.within(&name, *boundary),
            Kind::Extension(ext) => {
                *name == *ext.folded
                    || (name.len() > ext.folded.len() && name.ends_with(&ext.folded))
            }
            Kind::Glob(p) => p.full_match(&name),
            Kind::Path(_) => unreachable!(),
        }
    }

    /// Whether `matches` reads `is_dir` (a `dir/` query).
    pub fn needs_kind(&self) -> bool {
        matches!(&self.kind, Kind::Path(p) if p.dir_only)
    }

    pub fn names(&self, rel: &Path) -> Named {
        match &self.kind {
            Kind::Path(p) => p.names(rel),
            _ => {
                let Some(name) = rel.file_name() else {
                    return Named::No;
                };
                let name = fold(name.as_encoded_bytes());
                let spelled = match &self.kind {
                    Kind::Name { needle, .. } | Kind::Extension(needle) => *name == *needle.folded,
                    Kind::Glob(p) => p.names(&name),
                    Kind::Path(_) => unreachable!(),
                };
                if spelled { Named::Leaf } else { Named::No }
            }
        }
    }
}

fn is_separator(b: u8) -> bool {
    b == b'/' || (cfg!(windows) && b == b'\\')
}

fn is_glob(bytes: &[u8]) -> bool {
    bytes.iter().any(|&b| b == b'*' || b == b'?')
}

#[derive(Debug, Clone)]
struct PathQuery {
    components: Vec<Part>,
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
                _ => components.push(Part::new(part)),
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

    /// `Reach` for every component the query passes through; a `dir/` query's last component
    /// is only a `Leaf`, since nothing below it is a result.
    fn names(&self, rel: &Path) -> Named {
        let last = self.components.len() - 1;
        (0..self.components.len())
            .filter(|&i| self.tail_matches(rel, i + 1))
            .map(|i| {
                if i == last && self.dir_only {
                    Named::Leaf
                } else {
                    Named::Reach
                }
            })
            .max()
            .unwrap_or(Named::No)
    }

    /// Whether `rel` ends with the first `len` query components (anchored: consists of exactly
    /// them). Intermediate components need equality; the query's last component is a prefix of
    /// the entry name, or equal to it for a `dir/` query. Glob components match whole names.
    fn tail_matches(&self, rel: &Path, len: usize) -> bool {
        let last = self.components.len() - 1;
        let mut names = rel.components().rev().filter_map(|c| match c {
            Component::Normal(n) => Some(n),
            _ => None,
        });
        for (i, part) in self.components[..len].iter().enumerate().rev() {
            let Some(name) = names.next() else {
                return false;
            };
            let name = fold(name.as_encoded_bytes());
            let ok = match part {
                Part::Glob(p) => p.full_match(&name),
                Part::Literal(n) if i == last && !self.dir_only => name.starts_with(&n.folded),
                Part::Literal(n) => *name == *n.folded,
            };
            if !ok {
                return false;
            }
        }
        !self.anchored || names.next().is_none()
    }
}

#[derive(Debug, Clone)]
enum Part {
    Literal(Needle),
    Glob(Pattern),
}

impl Part {
    fn new(raw: &[u8]) -> Self {
        if is_glob(raw) {
            Self::Glob(Pattern::new(raw))
        } else {
            Self::Literal(Needle::new(raw))
        }
    }
}

/// Case-folded query text, compared with case-folded entry names as bytes.
#[derive(Debug, Clone)]
struct Needle {
    folded: Vec<u8>,
}

impl Needle {
    fn new(raw: &[u8]) -> Self {
        Self {
            folded: fold(raw).into_owned(),
        }
    }

    /// Substring of `hay`; with `boundary`, the match must start the name or follow a
    /// character that is not a letter or digit (`main.rs` in `my-main.rs`, not `domain.rs`).
    fn within(&self, hay: &[u8], boundary: bool) -> bool {
        hay.windows(self.folded.len())
            .enumerate()
            .any(|(i, w)| w == self.folded && (!boundary || i == 0 || !is_word_byte(hay[i - 1])))
    }
}

/// Non-ASCII bytes count as letters, so a boundary never falls inside a character.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || !b.is_ascii()
}

/// A whole-name wildcard pattern: `*` is any run, `?` one character, everything else literal.
#[derive(Debug, Clone)]
struct Pattern {
    folded: Vec<u8>,
}

impl Pattern {
    fn new(raw: &[u8]) -> Self {
        Self {
            folded: fold(raw).into_owned(),
        }
    }

    fn full_match(&self, name: &[u8]) -> bool {
        let (p, s) = (self.folded.as_slice(), name);
        let (mut pi, mut si) = (0, 0);
        let mut star: Option<(usize, usize)> = None;
        while si < s.len() {
            match p.get(pi) {
                Some(b'*') => {
                    star = Some((pi, si));
                    pi += 1;
                }
                Some(b'?') => {
                    pi += 1;
                    si += char_len(s, si);
                }
                Some(&c) if c == s[si] => {
                    pi += 1;
                    si += 1;
                }
                _ => match star {
                    Some((sp, ss)) => {
                        let next = ss + char_len(s, ss);
                        star = Some((sp, next));
                        pi = sp + 1;
                        si = next;
                    }
                    None => return false,
                },
            }
        }
        p[pi..].iter().all(|&b| b == b'*')
    }

    /// A pattern that starts with literal text spells the names it matches; `*` alone does not.
    fn names(&self, name: &[u8]) -> bool {
        !matches!(self.folded.first(), Some(b'*' | b'?')) && self.full_match(name)
    }
}

/// Bytes of the character starting at `i`: UTF-8 continuation bytes ride with their lead.
fn char_len(s: &[u8], i: usize) -> usize {
    1 + s[i + 1..].iter().take_while(|&&b| b & 0xC0 == 0x80).count()
}

/// Unicode lowercase plus NFC for UTF-8 text, so decomposed names (macOS, some archives) match
/// typed text; ASCII-only folding otherwise, so non-UTF-8 names are never decoded lossily.
fn fold(bytes: &[u8]) -> Cow<'_, [u8]> {
    if bytes.is_ascii() {
        if bytes.iter().any(u8::is_ascii_uppercase) {
            Cow::Owned(bytes.to_ascii_lowercase())
        } else {
            Cow::Borrowed(bytes)
        }
    } else if let Ok(s) = std::str::from_utf8(bytes) {
        Cow::Owned(s.to_lowercase().nfc().collect::<String>().into_bytes())
    } else {
        Cow::Owned(bytes.to_ascii_lowercase())
    }
}
