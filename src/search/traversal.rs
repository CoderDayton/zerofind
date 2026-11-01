//! Walk coordination: parallel workers send matches into a bounded channel.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::SyncSender;

use ignore::WalkState;

use super::filtering::{relative, walker};
use super::matcher;
use crate::domain::SearchPlan;
use crate::domain::scope::{RootKind, SearchRoot};

const MAX_THREADS: usize = 8;

/// Shared run state: `stop` ends the walk early; `errors` counts recoverable failures.
pub struct Control<'a> {
    pub stop: &'a AtomicBool,
    pub errors: &'a AtomicUsize,
}

pub fn run(plan: &SearchPlan, tx: SyncSender<PathBuf>, ctl: &Control<'_>) {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(MAX_THREADS));
    for root in &plan.roots {
        if ctl.stop.load(Ordering::Relaxed) {
            return;
        }
        match root.kind {
            RootKind::File => {
                let rel = root.path.file_name().map_or(root.path.as_path(), Path::new);
                if matcher::is_match(plan, rel, || false) && tx.send(root.path.clone()).is_err() {
                    return;
                }
            }
            RootKind::Dir => walk_dir(plan, root, threads, &tx, ctl),
        }
    }
}

fn walk_dir(
    plan: &SearchPlan,
    root: &SearchRoot,
    threads: usize,
    tx: &SyncSender<PathBuf>,
    ctl: &Control<'_>,
) {
    walker(root, plan, threads).run(|| {
        let tx = tx.clone();
        Box::new(move |result| visit(result, plan, root, &tx, ctl))
    });
}

fn visit(
    result: Result<ignore::DirEntry, ignore::Error>,
    plan: &SearchPlan,
    root: &SearchRoot,
    tx: &SyncSender<PathBuf>,
    ctl: &Control<'_>,
) -> WalkState {
    if ctl.stop.load(Ordering::Relaxed) {
        return WalkState::Quit;
    }
    let entry = match result {
        Ok(entry) => entry,
        Err(err) if is_rule_error(&err) => {
            eprintln!("zf: warning: {}", describe(&err, root));
            return WalkState::Continue;
        }
        Err(err) => {
            ctl.errors.fetch_add(1, Ordering::Relaxed);
            eprintln!("zf: {}", describe(&err, root));
            return WalkState::Continue;
        }
    };
    // Ignore-file problems in a walked directory ride on its entry rather than arriving as `Err`.
    if let Some(err) = entry.error() {
        eprintln!("zf: warning: {}", describe(err, root));
    }
    if entry.depth() == 0 {
        return WalkState::Continue;
    }
    let is_dir = || matcher::is_dir(entry.path(), entry.file_type());
    if matcher::is_match(plan, relative(entry.path(), &root.path), is_dir)
        && tx.send(root.display(entry.path()).to_path_buf()).is_err()
    {
        return WalkState::Quit;
    }
    WalkState::Continue
}

/// A malformed ignore rule: the walk still completed, so it warns without making the exit
/// status 3 (decision 0001 #11).
fn is_rule_error(err: &ignore::Error) -> bool {
    match err {
        ignore::Error::Glob { .. } => true,
        ignore::Error::Partial(errs) => errs.iter().all(is_rule_error),
        ignore::Error::WithPath { err, .. }
        | ignore::Error::WithDepth { err, .. }
        | ignore::Error::WithLineNumber { err, .. } => is_rule_error(err),
        _ => false,
    }
}

/// `path: reason`, with the path printed the way results are.
fn describe(err: &ignore::Error, root: &SearchRoot) -> String {
    match err {
        ignore::Error::WithPath { path, err } => {
            format!("{}: {}", root.display(path).display(), describe(err, root))
        }
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            describe(err, root)
        }
        other => other.to_string(),
    }
}
