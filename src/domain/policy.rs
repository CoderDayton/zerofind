//! Visibility and result-kind decisions.

use std::ffi::OsStr;

/// Built-in exclusions, version 1. Directories only; `--all` lifts them.
pub const EXCLUDED_DIRS_V1: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityPolicy {
    pub all: bool,
}

impl VisibilityPolicy {
    /// Whether an entry below the root may be emitted or descended into. `named_hidden` is the
    /// query's explicit-hidden exemption; it never lifts the built-in exclusions.
    /// `is_dir` is asked only for names on the exclusion list.
    pub fn admits(&self, name: &OsStr, is_dir: impl FnOnce() -> bool, named_hidden: bool) -> bool {
        if self.all {
            return true;
        }
        if name.to_str().is_some_and(|n| EXCLUDED_DIRS_V1.contains(&n)) && is_dir() {
            return false;
        }
        named_hidden || !name.as_encoded_bytes().starts_with(b".")
    }
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

    fn admits(p: VisibilityPolicy, name: &str, is_dir: bool) -> bool {
        p.admits(OsStr::new(name), || is_dir, false)
    }

    #[test]
    fn default_policy_table() {
        for name in [
            ".git",
            ".svn",
            ".hg",
            "node_modules",
            "target",
            "dist",
            "build",
            ".next",
        ] {
            assert!(!admits(DEFAULT, name, true), "{name} dir is excluded");
        }
        for name in [".env", ".cache", ".config"] {
            assert!(!admits(DEFAULT, name, false), "{name} is hidden");
            assert!(!admits(DEFAULT, name, true), "{name} is hidden");
        }
        for name in ["src", "tests", "README.md"] {
            assert!(
                admits(DEFAULT, name, name != "README.md"),
                "{name} is included"
            );
        }
    }

    #[test]
    fn exclusions_apply_to_directories_only() {
        assert!(
            admits(DEFAULT, "build", false),
            "a file named build is kept"
        );
        assert!(admits(DEFAULT, "target", false));
    }

    #[test]
    fn all_admits_everything() {
        for name in [".git", "node_modules", ".env", "src"] {
            assert!(admits(ALL, name, true));
        }
    }

    #[test]
    fn named_hidden_lifts_hidden_rule_only() {
        assert!(DEFAULT.admits(OsStr::new(".github"), || true, true));
        assert!(
            !DEFAULT.admits(OsStr::new(".git"), || true, true),
            "built-in exclusion is stronger"
        );
    }

    #[test]
    fn result_kind_filters_emitted_kinds() {
        assert!(ResultKind::Any.admits(true) && ResultKind::Any.admits(false));
        assert!(ResultKind::Files.admits(false) && !ResultKind::Files.admits(true));
        assert!(ResultKind::Dirs.admits(true) && !ResultKind::Dirs.admits(false));
    }
}
