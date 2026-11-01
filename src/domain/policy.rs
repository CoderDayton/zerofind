//! Visibility and result-kind decisions.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use super::query::Named;

/// Directories that are never worth searching: version control, dependencies, caches.
pub const ALWAYS_SKIPPED: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".next",
    "node_modules",
    "__pycache__",
];

const BUILD_MANIFESTS: &[&str] = &[
    "package.json",
    "pyproject.toml",
    "setup.py",
    "CMakeLists.txt",
    "Makefile",
    "meson.build",
    "build.gradle",
    "build.gradle.kts",
    "pom.xml",
    "go.mod",
    "Cargo.toml",
];

/// Directories skipped only beside a file that marks them as generated, so a hand-made
/// `~/Documents/build` stays searchable.
pub const SKIPPED_BESIDE: &[(&str, &[&str])] = &[
    ("target", &["Cargo.toml", "pom.xml"]),
    ("build", BUILD_MANIFESTS),
    ("dist", BUILD_MANIFESTS),
    ("out", BUILD_MANIFESTS),
    ("coverage", BUILD_MANIFESTS),
    ("vendor", &["go.mod", "composer.json"]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityPolicy {
    pub all: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Denied,
    /// Admitted by the default rules.
    Normal,
    /// Hidden or generated, admitted only because the query names it.
    Named(Named),
}

impl VisibilityPolicy {
    /// Whether an entry below the root may be emitted or descended into. `path` locates
    /// generated-directory markers; `is_dir` and `named` run only for hidden or listed names.
    pub fn admit(
        &self,
        path: &Path,
        name: &OsStr,
        is_dir: impl FnOnce() -> bool,
        named: impl FnOnce() -> Named,
    ) -> Admission {
        if self.all {
            return Admission::Normal;
        }
        let hidden = name.as_encoded_bytes().starts_with(b".");
        if !hidden && !(is_candidate(name) && is_dir() && is_generated(path, name)) {
            return Admission::Normal;
        }
        match named() {
            Named::No => Admission::Denied,
            n => Admission::Named(n),
        }
    }
}

fn is_candidate(name: &OsStr) -> bool {
    let Some(n) = name.to_str() else {
        return false;
    };
    ALWAYS_SKIPPED.contains(&n) || SKIPPED_BESIDE.iter().any(|(d, _)| *d == n) || mentions_env(n)
}

fn mentions_env(name: &str) -> bool {
    name.as_bytes()
        .windows(3)
        .any(|w| w.eq_ignore_ascii_case(b"env"))
}

/// `path` is a directory named `name`. A Python virtual environment is recognized by its
/// `pyvenv.cfg`, whatever it is called.
fn is_generated(path: &Path, name: &OsStr) -> bool {
    let Some(n) = name.to_str() else {
        return false;
    };
    if ALWAYS_SKIPPED.contains(&n) {
        return true;
    }
    let exists = |p: &Path| fs::symlink_metadata(p).is_ok();
    if let Some((_, markers)) = SKIPPED_BESIDE.iter().find(|(d, _)| *d == n) {
        let parent = path.parent().unwrap_or(Path::new("."));
        if markers.iter().any(|m| exists(&parent.join(m))) {
            return true;
        }
    }
    mentions_env(n) && exists(&path.join("pyvenv.cfg"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultKind {
    Any,
    Files,
    Dirs,
}

impl ResultKind {
    pub fn admits(&self, is_dir: bool) -> bool {
        match self {
            Self::Any => true,
            Self::Files => !is_dir,
            Self::Dirs => is_dir,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: VisibilityPolicy = VisibilityPolicy { all: false };
    const ALL: VisibilityPolicy = VisibilityPolicy { all: true };

    fn tree(entries: &[&str]) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        for e in entries {
            let p = t.path().join(e.trim_end_matches('/'));
            if e.ends_with('/') {
                fs::create_dir_all(p).unwrap();
            } else {
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(p, "").unwrap();
            }
        }
        t
    }

    fn admit(p: VisibilityPolicy, root: &Path, rel: &str, named: Named) -> Admission {
        let path = root.join(rel);
        let name = path.file_name().unwrap().to_owned();
        p.admit(&path, &name, || path.is_dir(), || named)
    }

    #[test]
    fn always_skipped_and_hidden_directories() {
        let t = tree(&["node_modules/", "__pycache__/", ".git/", ".cache/", "src/"]);
        for d in ["node_modules", "__pycache__", ".git", ".cache"] {
            assert_eq!(
                admit(DEFAULT, t.path(), d, Named::No),
                Admission::Denied,
                "{d}"
            );
        }
        assert_eq!(
            admit(DEFAULT, t.path(), "src", Named::No),
            Admission::Normal
        );
    }

    #[test]
    fn build_directories_need_a_marker() {
        let t = tree(&[
            "notes/build/",
            "notes/dist/",
            "app/package.json",
            "app/build/",
            "app/dist/",
        ]);
        for d in ["notes/build", "notes/dist"] {
            assert_eq!(
                admit(DEFAULT, t.path(), d, Named::No),
                Admission::Normal,
                "{d}"
            );
        }
        for d in ["app/build", "app/dist"] {
            assert_eq!(
                admit(DEFAULT, t.path(), d, Named::No),
                Admission::Denied,
                "{d}"
            );
        }
    }

    #[test]
    fn target_needs_a_cargo_or_maven_project() {
        let t = tree(&[
            "shoot/target/",
            "rs/Cargo.toml",
            "rs/target/",
            "mvn/pom.xml",
            "mvn/target/",
        ]);
        assert_eq!(
            admit(DEFAULT, t.path(), "shoot/target", Named::No),
            Admission::Normal
        );
        assert_eq!(
            admit(DEFAULT, t.path(), "rs/target", Named::No),
            Admission::Denied
        );
        assert_eq!(
            admit(DEFAULT, t.path(), "mvn/target", Named::No),
            Admission::Denied
        );
    }

    #[test]
    fn vendor_needs_go_or_composer() {
        let t = tree(&["a/vendor/", "go/go.mod", "go/vendor/"]);
        assert_eq!(
            admit(DEFAULT, t.path(), "a/vendor", Named::No),
            Admission::Normal
        );
        assert_eq!(
            admit(DEFAULT, t.path(), "go/vendor", Named::No),
            Admission::Denied
        );
    }

    #[test]
    fn virtual_environments_are_recognized_by_pyvenv_cfg() {
        let t = tree(&[
            "venv/pyvenv.cfg",
            "py-env/pyvenv.cfg",
            "environment/",
            "venv2/",
        ]);
        assert_eq!(
            admit(DEFAULT, t.path(), "venv", Named::No),
            Admission::Denied
        );
        assert_eq!(
            admit(DEFAULT, t.path(), "py-env", Named::No),
            Admission::Denied
        );
        assert_eq!(
            admit(DEFAULT, t.path(), "environment", Named::No),
            Admission::Normal
        );
        assert_eq!(
            admit(DEFAULT, t.path(), "venv2", Named::No),
            Admission::Normal
        );
    }

    #[test]
    fn listed_names_that_are_files_are_kept() {
        let t = tree(&["Cargo.toml", "target", "node_modules"]);
        assert_eq!(
            admit(DEFAULT, t.path(), "target", Named::No),
            Admission::Normal
        );
        assert_eq!(
            admit(DEFAULT, t.path(), "node_modules", Named::No),
            Admission::Normal
        );
    }

    #[test]
    fn a_named_entry_is_admitted_with_its_naming() {
        let t = tree(&["node_modules/", ".env"]);
        assert_eq!(
            admit(DEFAULT, t.path(), "node_modules", Named::Leaf),
            Admission::Named(Named::Leaf)
        );
        assert_eq!(
            admit(DEFAULT, t.path(), ".env", Named::Reach),
            Admission::Named(Named::Reach)
        );
    }

    #[test]
    fn all_admits_everything() {
        let t = tree(&["node_modules/", ".env", "venv/pyvenv.cfg"]);
        for name in ["node_modules", ".env", "venv"] {
            assert_eq!(admit(ALL, t.path(), name, Named::No), Admission::Normal);
        }
    }

    #[test]
    fn result_kind_filters_emitted_kinds() {
        assert!(ResultKind::Any.admits(true) && ResultKind::Any.admits(false));
        assert!(ResultKind::Files.admits(false) && !ResultKind::Files.admits(true));
        assert!(ResultKind::Dirs.admits(true) && !ResultKind::Dirs.admits(false));
    }
}
