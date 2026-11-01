//! Visibility and ignore evaluation, applied before descent.

use std::path::Path;

use ignore::{DirEntry, WalkBuilder, WalkParallel};

use super::matcher;
use crate::domain::SearchPlan;
use crate::domain::policy::VisibilityPolicy;
use crate::domain::query::CompiledQuery;
use crate::domain::scope::SearchRoot;

/// A walker with every option set explicitly, so no dependency default leaks into
/// behavior.
pub fn walker(root: &SearchRoot, plan: &SearchPlan, threads: usize) -> WalkParallel {
    let rules = !plan.visibility.all;
    let mut builder = WalkBuilder::new(&root.path);
    builder
        .standard_filters(false)
        // Hidden entries are decided by VisibilityPolicy, which knows the exemption for hidden names in path queries.
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
    builder.filter_entry(move |entry| admits(entry, &root_path, &query, policy));
    builder.build_parallel()
}

fn admits(entry: &DirEntry, root: &Path, query: &CompiledQuery, policy: VisibilityPolicy) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name();
    // Same kind as matching uses, so exclusion and `--dirs` agree about symlinks.
    let is_dir = || matcher::is_dir(entry.path(), entry.file_type());
    let named_hidden = !policy.all
        && name.as_encoded_bytes().starts_with(b".")
        && query.names_hidden(relative(entry.path(), root));
    policy.admits(name, is_dir, named_hidden)
}

pub fn relative<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}
