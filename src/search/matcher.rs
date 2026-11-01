//! Per-entry evaluation: query match plus emitted kind.

use std::fs::{self, FileType};
use std::path::Path;

use crate::domain::SearchPlan;
use crate::domain::policy::ResultKind;

/// `is_dir` runs only when the plan depends on the entry kind, so symlinks are not stat'ed
/// needlessly.
pub fn is_match(plan: &SearchPlan, rel: &Path, is_dir: impl FnOnce() -> bool) -> bool {
    let is_dir = if plan.result_kind != ResultKind::Any || plan.query.needs_kind() {
        is_dir()
    } else {
        false
    };
    plan.result_kind.admits(is_dir) && plan.query.matches(rel, is_dir)
}

/// The kind used for matching and `--files`/`--dirs`. A symlink takes its target's kind; it is
/// still never descended into.
pub fn is_dir(path: &Path, file_type: Option<FileType>) -> bool {
    match file_type {
        Some(t) if t.is_symlink() => fs::metadata(path).is_ok_and(|m| m.is_dir()),
        Some(t) => t.is_dir(),
        None => false,
    }
}
