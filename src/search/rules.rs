//! Ignore files across a walk, applied as the `ignore` crate's walker applies them.
//!
//! `.ignore` applies everywhere. `.gitignore`, `.git/info/exclude` and the user's global git
//! ignore file apply only inside a repository (`.git`, or `.jj`, which reads them too), and git
//! rules from above a repository's top do not reach into it. Folders above the search root
//! contribute too. tests/walk_oracle.rs holds this to the crate's own walker. One deliberate
//! difference: a worktree's relative `gitdir:` resolves from the worktree, as git does.

use std::fs::{self, DirEntry, FileType};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::Error;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// The rules in force in one directory: a stack of layers, innermost first. Directories that
/// add nothing share their parent's stack.
#[derive(Clone, Default)]
pub struct Rules(Option<Arc<Layer>>);

struct Layer {
    parent: Rules,
    ignore: Gitignore,
    git: Gitignore,
    exclude: Gitignore,
    /// Holds a `.git` or `.jj`.
    repo: bool,
    /// This layer or one below the root of the stack is a repository.
    in_repo: bool,
    /// A folder above the search root: it matches paths under the root's canonical path.
    above_root: bool,
}

/// Which ignore-related names a directory listing holds; `PROBE` checks the filesystem.
#[derive(Clone, Copy, Default)]
struct Found {
    ignore: bool,
    gitignore: bool,
    git: bool,
    jj: bool,
}

impl Found {
    const PROBE: Self = Self {
        ignore: true,
        gitignore: true,
        git: true,
        jj: true,
    };

    fn of(entries: &[DirEntry]) -> Self {
        let mut f = Self::default();
        for e in entries {
            match e.file_name().as_encoded_bytes() {
                b".ignore" => f.ignore = true,
                b".gitignore" => f.gitignore = true,
                b".git" => f.git = true,
                b".jj" => f.jj = true,
                _ => {}
            }
        }
        f
    }
}

impl Rules {
    /// Rules from the folders above the root, given its canonical path. Problems in their
    /// ignore files go to `problems`.
    pub fn above(canon: &Path, problems: &mut Vec<Error>) -> Self {
        let parents: Vec<&Path> = canon.ancestors().skip(1).collect();
        parents
            .into_iter()
            .rev()
            .fold(Self::default(), |rules, dir| {
                rules.child(dir, Found::PROBE, true, problems)
            })
    }

    /// Rules for `dir`, given its listing.
    pub fn enter(&self, dir: &Path, entries: &[DirEntry], problems: &mut Vec<Error>) -> Self {
        self.child(dir, Found::of(entries), false, problems)
    }

    fn child(&self, dir: &Path, found: Found, above_root: bool, problems: &mut Vec<Error>) -> Self {
        let git_type = if found.git {
            fs::metadata(dir.join(".git")).ok().map(|m| m.file_type())
        } else {
            None
        };
        let repo = git_type.is_some() || (found.jj && dir.join(".jj").exists());
        let ignore = if found.ignore {
            load(dir, dir, ".ignore", problems)
        } else {
            Gitignore::empty()
        };
        let git = if found.gitignore {
            load(dir, dir, ".gitignore", problems)
        } else {
            Gitignore::empty()
        };
        let exclude = if found.git {
            match common_dir(dir, git_type) {
                Ok(common) => load(dir, &common, "info/exclude", problems),
                Err(err) => {
                    problems.extend(err);
                    Gitignore::empty()
                }
            }
        } else {
            Gitignore::empty()
        };
        if !repo && ignore.is_empty() && git.is_empty() && exclude.is_empty() {
            return self.clone();
        }
        Self(Some(Arc::new(Layer {
            in_repo: repo || self.in_repo(),
            parent: self.clone(),
            ignore,
            git,
            exclude,
            repo,
            above_root,
        })))
    }

    fn in_repo(&self) -> bool {
        self.0.as_ref().is_some_and(|l| l.in_repo)
    }

    fn layers(&self) -> impl Iterator<Item = &Layer> {
        std::iter::successors(self.0.as_deref(), |l| l.parent.0.as_deref())
    }
}

/// Matches walked paths against a [`Rules`] stack for one search root.
pub struct Matcher<'a> {
    global: &'a Gitignore,
    root: &'a Path,
    canon: Option<PathBuf>,
}

impl<'a> Matcher<'a> {
    pub fn new(global: &'a Gitignore, root: &'a Path) -> Self {
        Self {
            global,
            root,
            canon: root.canonicalize().ok(),
        }
    }

    /// The root's canonical path; `None` if it could not be resolved.
    pub fn canon(&self) -> Option<&Path> {
        self.canon.as_deref()
    }

    /// The nearest `.ignore` rule decides; then the nearest git rule up to the repository's
    /// top, then `info/exclude`, then the global file. `is_dir` is the entry's own kind: a
    /// symlink is never a directory here.
    pub fn ignored(&self, rules: &Rules, path: &Path, is_dir: bool) -> bool {
        let in_repo = rules.in_repo();
        let (mut git, mut exclude) = (None, None);
        let mut saw_repo = false;
        let mut above = None;
        for layer in rules.layers() {
            let check_git = in_repo && !saw_repo;
            saw_repo |= layer.repo;
            // Skip layers with nothing to check here before building an above-root path.
            let git_rules = check_git && !(layer.git.is_empty() && layer.exclude.is_empty());
            if layer.ignore.is_empty() && !git_rules {
                continue;
            }
            let p = if layer.above_root {
                match above.get_or_insert_with(|| self.above_path(path)) {
                    Some(p) => p.as_path(),
                    None => break,
                }
            } else {
                path
            };
            let m = layer.ignore.matched(p, is_dir);
            if !m.is_none() {
                return m.is_ignore();
            }
            if check_git {
                if git.is_none() {
                    let m = layer.git.matched(p, is_dir);
                    git = (!m.is_none()).then(|| m.is_ignore());
                }
                if exclude.is_none() {
                    let m = layer.exclude.matched(p, is_dir);
                    exclude = (!m.is_none()).then(|| m.is_ignore());
                }
            }
        }
        git.or(exclude)
            .unwrap_or_else(|| in_repo && self.global.matched(path, is_dir).is_ignore())
    }

    /// `path` under the root's canonical path, as rules from above the root see it.
    fn above_path(&self, path: &Path) -> Option<PathBuf> {
        let rel = path.strip_prefix(self.root).ok()?;
        Some(self.canon.as_ref()?.join(rel))
    }
}

/// One ignore file, rooted at `dir`. A missing or unreadable file is no rules; a bad pattern
/// is a problem and the rest of the file still applies.
fn load(dir: &Path, from: &Path, name: &str, problems: &mut Vec<Error>) -> Gitignore {
    let file = from.join(name);
    let mut builder = GitignoreBuilder::new(dir);
    if cfg!(windows) || file.exists() {
        problems.extend(builder.add(&file).filter(|e| !e.is_io()));
    }
    builder.build().unwrap_or_else(|err| {
        problems.push(err);
        Gitignore::empty()
    })
}

/// The git directory holding `info/exclude`. A `.git` file (worktree or submodule) points to
/// its git directory, whose `commondir` file names the shared one.
fn common_dir(dir: &Path, git_type: Option<FileType>) -> Result<PathBuf, Option<Error>> {
    let dot_git = dir.join(".git");
    if !git_type.is_some_and(|t| t.is_file()) {
        return Ok(dot_git);
    }
    let line = first_line(&dot_git)
        .map_err(|e| Some(io_error(e, &dot_git)))?
        .ok_or(None)?;
    // A relative gitdir (`git worktree add --relative-paths`) is relative to this directory;
    // the `ignore` crate resolves it against the working directory instead.
    let git_dir = dir.join(line.strip_prefix("gitdir: ").ok_or(None)?);
    // A git directory without `commondir` is not a linked worktree: no shared exclude file.
    let line = first_line(&git_dir.join("commondir"))
        .ok()
        .flatten()
        .ok_or(None)?;
    Ok(if line.starts_with('.') {
        git_dir.join(line)
    } else {
        PathBuf::from(line)
    })
}

fn first_line(path: &Path) -> io::Result<Option<String>> {
    BufReader::new(fs::File::open(path)?)
        .lines()
        .next()
        .transpose()
}

fn io_error(err: io::Error, path: &Path) -> Error {
    Error::WithPath {
        path: path.to_path_buf(),
        err: Box::new(Error::Io(err)),
    }
}
