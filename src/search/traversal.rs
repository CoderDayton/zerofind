//! Walk coordination: parallel workers push matches into their own output buffers.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ignore::WalkState;

use super::filtering::{admission, relative, walker};
use super::matcher;
use crate::domain::SearchPlan;
use crate::domain::policy::Admission;
use crate::domain::query::Named;
use crate::domain::scope::{RootKind, SearchRoot};
use crate::output::diag;
use crate::output::sink::{Producer, Sink};

/// Chosen by a sweep of 2–32 workers; see docs/performance.md#worker-count.
const MAX_THREADS: usize = 8;

pub fn worker_count() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(MAX_THREADS))
}

/// Shared run state: `stop` ends this walk early, `interrupted` is the process-wide Ctrl+C
/// flag, and `errors` counts recoverable failures.
pub struct Control<'a> {
    pub stop: &'a AtomicBool,
    pub interrupted: &'a AtomicBool,
    pub errors: &'a AtomicUsize,
}

impl Control<'_> {
    fn halted(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || self.interrupted.load(Ordering::Relaxed)
    }
}

pub fn run(plan: &SearchPlan, threads: usize, sink: &Sink, ctl: &Control<'_>) {
    for root in &plan.roots {
        if ctl.halted() {
            return;
        }
        match root.kind {
            RootKind::File => {
                let rel = root.path.file_name().map_or(root.path.as_path(), Path::new);
                if matcher::is_match(plan, rel, || false)
                    && sink
                        .producer()
                        .push(root.path.as_os_str().as_encoded_bytes())
                        .is_err()
                {
                    return;
                }
            }
            RootKind::Dir => walk_dir(plan, root, threads, sink, ctl),
        }
    }
}

fn walk_dir(plan: &SearchPlan, root: &SearchRoot, threads: usize, sink: &Sink, ctl: &Control<'_>) {
    walker(root, plan, threads).run(|| {
        let out = sink.producer();
        Box::new(move |result| visit(result, plan, root, &out, ctl))
    });
}

fn visit(
    result: Result<ignore::DirEntry, ignore::Error>,
    plan: &SearchPlan,
    root: &SearchRoot,
    out: &Producer<'_>,
    ctl: &Control<'_>,
) -> WalkState {
    if ctl.halted() {
        return WalkState::Quit;
    }
    let entry = match result {
        Ok(entry) => entry,
        Err(err) if is_rule_error(&err) => {
            diag::warning(describe(&err, root));
            return WalkState::Continue;
        }
        Err(err) => {
            ctl.errors.fetch_add(1, Ordering::Relaxed);
            diag::error(describe(&err, root));
            return WalkState::Continue;
        }
    };
    // Ignore-file problems in a walked directory ride on its entry rather than arriving as `Err`.
    if let Some(err) = entry.error() {
        diag::warning(describe(err, root));
    }
    if entry.depth() == 0 {
        return WalkState::Continue;
    }
    let is_dir = || matcher::is_dir(entry.path(), entry.file_type());
    if matcher::is_match(plan, relative(entry.path(), &root.path), is_dir)
        && out
            .push(root.display(entry.path()).as_os_str().as_encoded_bytes())
            .is_err()
    {
        return WalkState::Quit;
    }
    // A hidden or generated directory the query only spells is a result, not a place to look.
    // Symlinks are never entered, so only real directories need the check.
    if entry.file_type().is_some_and(|t| t.is_dir())
        && admission(&entry, &root.path, &plan.query, plan.visibility)
            == Admission::Named(Named::Leaf)
    {
        return WalkState::Skip;
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
