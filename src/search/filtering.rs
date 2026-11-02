//! Visibility evaluation, applied before descent.

use std::ffi::OsStr;
use std::fs::FileType;
use std::path::Path;

use super::matcher;
use crate::domain::policy::{Admission, VisibilityPolicy};
use crate::domain::query::Query;

/// Whether an entry below the root is walked normally, only reachable by name, or skipped.
pub fn admission(
    path: &Path,
    name: &OsStr,
    file_type: FileType,
    rel: &Path,
    query: &Query,
    policy: VisibilityPolicy,
) -> Admission {
    // Same kind as matching uses, so exclusion and `--dirs` agree about symlinks.
    let is_dir = || matcher::is_dir(path, Some(file_type));
    policy.admit(path, name, is_dir, || query.names(rel))
}

pub fn relative<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}
