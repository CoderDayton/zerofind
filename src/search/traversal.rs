//! Walk coordination: workers read directories, apply ignore rules and visibility, match, and
//! push results into their own output buffers.

use std::ffi::OsStr;
use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use ignore::gitignore::Gitignore;

use super::exclude;
use super::filtering::relative;
use super::matcher;
use super::rules::{Matcher, Rules};
use super::walk;
use crate::domain::SearchPlan;
use crate::domain::policy::Admission;
use crate::domain::query::Named;
use crate::domain::scope::{RootKind, SearchRoot};
use crate::output::diag;
use crate::output::sink::{Producer, Sink};

/// Workers grow with the backlog up to this many. Chosen by warm and cold sweeps of 2–32
/// workers; see docs/performance.md#worker-count.
const MAX_THREADS: usize = 16;

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
    let trace = std::env::var_os("ZF_TRACE").is_some_and(|v| v == "1");
    // Read once per run; problems in the user's global file are not this search's to report.
    let global = (!plan.visibility.all).then(|| Gitignore::global().0);
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
            RootKind::Dir => {
                let start = Instant::now();
                let stats = walk_dir(plan, root, threads, global.as_ref(), sink, ctl);
                if trace {
                    diag::trace(format_args!(
                        "{}: workers={} dirs={} entries={} waits={} ms={:.1}",
                        root.path.display(),
                        stats.workers,
                        stats.dirs,
                        stats.entries,
                        stats.waits,
                        start.elapsed().as_secs_f64() * 1e3,
                    ));
                }
            }
        }
    }
}

/// A directory to read, with the ignore rules in force there.
struct Job {
    dir: PathBuf,
    rules: Rules,
}

fn walk_dir(
    plan: &SearchPlan,
    root: &SearchRoot,
    threads: usize,
    global: Option<&Gitignore>,
    sink: &Sink,
    ctl: &Control<'_>,
) -> walk::Stats {
    let matcher = global.map(|g| Matcher::new(g, &root.path));
    let mut problems = Vec::new();
    let rules = match matcher.as_ref().and_then(Matcher::canon) {
        Some(canon) => Rules::above(canon, &mut problems),
        None => Rules::default(),
    };
    warn_all(problems, root);
    // Validated when the plan was built.
    let exclude = exclude::build(&root.path, &plan.exclude).ok().flatten();
    let first = Job {
        dir: root.path.clone(),
        rules,
    };
    let walker = Walker {
        plan,
        root,
        matcher: matcher.as_ref(),
        exclude: exclude.as_ref(),
        ctl,
    };
    walk::run(
        first,
        threads,
        &|| sink.producer(),
        &|out, job, found| walker.read(out, job, found),
        &|| ctl.halted(),
    )
}

struct Walker<'a> {
    plan: &'a SearchPlan,
    root: &'a SearchRoot,
    /// `None` with `--all`: no ignore rules.
    matcher: Option<&'a Matcher<'a>>,
    /// `--exclude` patterns: applied before every other rule, with or without `--all`.
    exclude: Option<&'a Gitignore>,
    ctl: &'a Control<'a>,
}

impl Walker<'_> {
    /// Lists one directory, emits its matches and queues the subdirectories to enter. Returns
    /// the number of entries listed.
    fn read(&self, out: &mut Producer<'_>, job: Job, found: &mut Vec<Job>) -> usize {
        let entries = match fs::read_dir(&job.dir) {
            Ok(entries) => entries,
            Err(err) => {
                self.fail(&job.dir, "cannot read directory", &err);
                return 0;
            }
        };
        // The whole listing first: this directory's ignore files apply to its own entries.
        let mut list = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => list.push(entry),
                Err(err) => self.fail(&job.dir, "cannot read directory", &err),
            }
        }
        let rules = match self.matcher {
            Some(_) => {
                let mut problems = Vec::new();
                let rules = job.rules.enter(&job.dir, &list, &mut problems);
                warn_all(problems, self.root);
                rules
            }
            None => job.rules,
        };
        // One path buffer for the whole listing: each entry swaps in its name, so only the
        // directories queued below get a path of their own.
        let mut path = job.dir.join("_");
        let base = path.as_os_str().len() - 1;
        for entry in &list {
            if self.ctl.halted() {
                break;
            }
            let name = entry.file_name();
            set_name(&mut path, base, &name);
            if self.visit(out, entry, &path, &name, &rules, found).is_err() {
                self.ctl.stop.store(true, Ordering::Relaxed);
                break;
            }
        }
        list.len()
    }

    fn visit(
        &self,
        out: &Producer<'_>,
        entry: &DirEntry,
        path: &Path,
        name: &OsStr,
        rules: &Rules,
        found: &mut Vec<Job>,
    ) -> Result<(), ()> {
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(err) => {
                self.fail(path, "cannot read file type", &err);
                return Ok(());
            }
        };
        // The entry's own kind: a symlink to a directory is not one here.
        let own_dir = file_type.is_dir();
        if self
            .exclude
            .is_some_and(|x| x.matched(path, own_dir).is_ignore())
        {
            return Ok(());
        }
        if self
            .matcher
            .is_some_and(|m| m.ignored(rules, path, own_dir))
        {
            return Ok(());
        }
        let plan = self.plan;
        let rel = relative(path, &self.root.path);
        // Same kind for visibility and matching, so exclusion and `--dirs` agree about symlinks.
        let is_dir = || matcher::is_dir(path, file_type);
        let admitted = plan
            .visibility
            .admit(path, name, is_dir, || plan.query.names(rel));
        if admitted == Admission::Denied {
            return Ok(());
        }
        if matcher::is_match(plan, rel, is_dir) {
            out.push(self.root.display(path).as_os_str().as_encoded_bytes())
                .map_err(|_| ())?;
        }
        // A hidden or generated directory the query only spells is a result, not a place to
        // look. Symlinks are never entered.
        if own_dir && admitted != Admission::Named(Named::Leaf) {
            found.push(Job {
                dir: path.to_path_buf(),
                rules: rules.clone(),
            });
        }
        Ok(())
    }

    /// `path: operation: reason`, with the path printed the way results are.
    fn fail(&self, path: &Path, operation: &str, err: &io::Error) {
        self.ctl.errors.fetch_add(1, Ordering::Relaxed);
        diag::error(format_args!(
            "{}: {operation}: {err}",
            self.root.display(path).display()
        ));
    }
}

/// Replaces everything after the first `base` bytes of `path` with `name`, reusing its buffer.
#[cfg(unix)]
fn set_name(path: &mut PathBuf, base: usize, name: &OsStr) {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let mut bytes = std::mem::take(path).into_os_string().into_vec();
    bytes.truncate(base);
    bytes.extend_from_slice(name.as_bytes());
    *path = OsString::from_vec(bytes).into();
}

#[cfg(not(unix))]
fn set_name(path: &mut PathBuf, _base: usize, name: &OsStr) {
    path.set_file_name(name);
}

/// Ignore-file problems warn without changing the exit status, one line each.
fn warn_all(problems: Vec<ignore::Error>, root: &SearchRoot) {
    for err in problems {
        match err {
            ignore::Error::Partial(errs) => warn_all(errs, root),
            err => diag::warning(describe(&err, root)),
        }
    }
}

/// `path: line N: reason`, with the path printed the way results are.
fn describe(err: &ignore::Error, root: &SearchRoot) -> String {
    match err {
        ignore::Error::WithPath { path, err } => {
            format!("{}: {}", root.display(path).display(), describe(err, root))
        }
        ignore::Error::WithDepth { err, .. } => describe(err, root),
        other => other.to_string(),
    }
}
