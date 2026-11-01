//! Visibility and ignore evaluation, applied before descent.

use std::path::Path;

use ignore::{DirEntry, WalkBuilder, WalkParallel};

use super::matcher;
use crate::domain::SearchPlan;
use crate::domain::policy::{Admission, VisibilityPolicy};
use crate::domain::query::Query;
use crate::domain::scope::SearchRoot;

/// A walker with every option set explicitly, so no dependency default leaks into
/// behavior.
pub fn walker(root: &SearchRoot, plan: &SearchPlan, threads: usize) -> WalkParallel {
    let rules = !plan.visibility.all;
    let mut builder = WalkBuilder::new(&root.path);
    builder
        .standard_filters(false)
        // Hidden entries are decided by VisibilityPolicy, which knows the names the query spells.
        .hidden(false)
        .parents(rules)
        .ignore(rules)
        .git_ignore(rules)
        .git_global(rules)
        .git_exclude(rules)
        // Git rules only inside a repository: a dotfiles `~/.gitignore` of `*` must not hide
        // everything below it. `.ignore` files apply everywhere.
        .require_git(true)
        .ignore_case_insensitive(false)
        .follow_links(false)
        .same_file_system(false)
        .max_depth(None)
        .max_filesize(None)
        .skip_stdout(false)
        .threads(threads);

    let root_path = root.path.clone();
    let query = plan.query.clone();
    let policy = plan.visibility;
    builder.filter_entry(move |entry| {
        admission(entry, &root_path, &query, policy) != Admission::Denied
    });
    builder.build_parallel()
}

pub fn admission(
    entry: &DirEntry,
    root: &Path,
    query: &Query,
    policy: VisibilityPolicy,
) -> Admission {
    if entry.depth() == 0 {
        return Admission::Normal;
    }
    // Same kind as matching uses, so exclusion and `--dirs` agree about symlinks.
    let is_dir = || matcher::is_dir(entry.path(), entry.file_type());
    let named = || query.names(relative(entry.path(), root));
    policy.admit(entry.path(), entry.file_name(), is_dir, named)
}

pub fn relative<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}
