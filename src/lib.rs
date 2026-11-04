//! ZeroFind: stream-first filesystem discovery.
//!
//! The library exists for the `zf` binary and its tests. Only [`run`] is supported; the hidden
//! modules are not a stable API.

#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod domain;
#[doc(hidden)]
pub mod error;
#[doc(hidden)]
pub mod output;
#[doc(hidden)]
pub mod search;

use std::io::{self, BufWriter, IsTerminal, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use clap::Parser;

use cli::Cli;
use domain::policy::{ResultKind, VisibilityPolicy};
use domain::query::{CompiledQuery, Query};
use domain::scope::{self, SearchRoot};
use domain::{Order, OutputFormat, SearchPlan};
use error::{ZfError, status};
use output::sink::{self, Ending, Finish, Sink};
use search::traversal::{self, Control};

/// Ctrl+C is process-wide, so its flag is too; everything else lives in one run.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub fn run() -> ExitCode {
    let cli = Cli::parse_from(cli::normalize(std::env::args_os()));
    // Without a handler, Ctrl+C still ends the process; it just skips the clean exit.
    let _ = ctrlc::set_handler(|| {
        if on_ctrl_c(&INTERRUPTED) {
            std::process::exit(status::INTERRUPTED.into());
        }
    });
    match plan(&cli).and_then(|plan| execute(&plan, io::stdout().lock(), &INTERRUPTED)) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            output::diag::error(&e);
            ExitCode::from(e.status())
        }
    }
}

/// Records a Ctrl+C. True on a repeat: the walk may be stuck in slow I/O, so the caller exits
/// at once instead of waiting for the clean stop.
fn on_ctrl_c(interrupted: &AtomicBool) -> bool {
    interrupted.swap(true, Ordering::Relaxed)
}

fn plan(cli: &Cli) -> Result<SearchPlan, ZfError> {
    let home = dirs::home_dir();
    let mut roots = Vec::new();
    let mut terms = Vec::new();
    for arg in &cli.terms {
        if scope::is_location(arg) {
            let loc = scope::resolve_location(arg, home.as_deref())?;
            roots.push(loc.root);
            terms.extend(loc.term);
        } else {
            terms.push(arg.clone());
        }
    }
    for path in &cli.path {
        roots.push(scope::resolve_explicit(path)?);
    }
    if cli.home {
        roots.push(scope::resolve_home(home.as_deref())?);
    }
    if roots.is_empty() {
        roots.push(SearchRoot::local()?);
    }
    let terms = terms
        .iter()
        .map(|t| CompiledQuery::compile(t))
        .collect::<Result<_, _>>()?;
    search::exclude::check(&cli.exclude)?;
    Ok(SearchPlan {
        roots: scope::dedupe(roots),
        query: Query::new(terms),
        visibility: VisibilityPolicy { all: cli.all },
        exclude: cli.exclude.clone(),
        result_kind: match (cli.files, cli.dirs) {
            (true, _) => ResultKind::Files,
            (_, true) => ResultKind::Dirs,
            _ => ResultKind::Any,
        },
        output: if cli.print0 {
            OutputFormat::Nul
        } else if io::stdout().is_terminal() {
            OutputFormat::Terminal
        } else {
            OutputFormat::Lines
        },
        order: match (cli.nearest, cli.modified) {
            (true, _) => Order::Nearest,
            (_, true) => Order::Modified,
            _ => Order::Found,
        },
    })
}

fn execute(plan: &SearchPlan, out: impl Write, interrupted: &AtomicBool) -> Result<u8, ZfError> {
    match plan.order {
        Order::Found => stream(plan, out, interrupted),
        order => sorted(plan, order, out, interrupted),
    }
}

/// Collects each root's results, sorts them and prints them, root by root in the order given.
/// Ctrl+C prints nothing more; only whole records are written.
fn sorted(
    plan: &SearchPlan,
    order: Order,
    out: impl Write,
    interrupted: &AtomicBool,
) -> Result<u8, ZfError> {
    let mut out = BufWriter::with_capacity(32 * 1024, out);
    let mut code = status::NO_MATCH;
    for root in &plan.roots {
        let one = SearchPlan {
            roots: vec![root.clone()],
            // NUL cannot occur in a path, so records split back apart exactly.
            output: OutputFormat::Nul,
            order: Order::Found,
            ..plan.clone()
        };
        let mut found = Vec::new();
        let root_code = stream(&one, &mut found, interrupted)?;
        if root_code == status::INTERRUPTED {
            return Ok(root_code);
        }
        let mut records: Vec<&[u8]> = found.split(|&b| b == 0).filter(|r| !r.is_empty()).collect();
        output::order::sort(&mut records, order, traversal::worker_count());
        match print(&mut out, &records, plan.output, interrupted) {
            Ok(true) => {}
            Ok(false) => return written(out.flush()).map(|()| status::INTERRUPTED),
            Err(e) => return written(Err(e)).map(|()| status::MATCHED),
        }
        code = match (code, root_code) {
            (status::PARTIAL, _) | (_, status::PARTIAL) => status::PARTIAL,
            (status::MATCHED, _) | (_, status::MATCHED) => status::MATCHED,
            _ => status::NO_MATCH,
        };
    }
    Ok(code)
}

/// Writes and flushes one root's records; `Ok(false)` when Ctrl+C stopped it first.
fn print(
    out: &mut impl Write,
    records: &[&[u8]],
    format: OutputFormat,
    interrupted: &AtomicBool,
) -> io::Result<bool> {
    let mut line = Vec::new();
    for record in records {
        if interrupted.load(Ordering::Relaxed) {
            return Ok(false);
        }
        line.clear();
        sink::encode(&mut line, record, format);
        out.write_all(&line)?;
    }
    out.flush()?;
    Ok(true)
}

/// A closed pipe is a quiet success, as in streaming mode; any other write error is fatal.
fn written(result: io::Result<()>) -> Result<(), ZfError> {
    match result {
        Err(e) if e.kind() != io::ErrorKind::BrokenPipe => Err(ZfError::Output(e)),
        _ => Ok(()),
    }
}

fn stream(plan: &SearchPlan, out: impl Write, interrupted: &AtomicBool) -> Result<u8, ZfError> {
    let stop = AtomicBool::new(false);
    let errors = AtomicUsize::new(0);
    let ctl = Control {
        stop: &stop,
        interrupted,
        errors: &errors,
    };
    let threads = traversal::worker_count();
    let sink = Sink::new(threads, plan.output);
    let written = std::thread::scope(|s| {
        s.spawn(|| {
            let _finish = Finish(&sink);
            traversal::run(plan, threads, &sink, &ctl);
        });
        let written = sink::drain(&sink, out, interrupted);
        stop.store(true, Ordering::Relaxed);
        written
    })
    .map_err(ZfError::Output)?;

    Ok(match written.ending {
        Ending::BrokenPipe => status::MATCHED,
        _ if interrupted.load(Ordering::Relaxed) => status::INTERRUPTED,
        _ if errors.load(Ordering::Relaxed) > 0 => status::PARTIAL,
        _ if written.any => status::MATCHED,
        _ => status::NO_MATCH,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_ctrl_c_asks_for_an_immediate_exit() {
        let flag = AtomicBool::new(false);
        assert!(!on_ctrl_c(&flag), "the first press stops cleanly");
        assert!(flag.load(Ordering::Relaxed));
        assert!(on_ctrl_c(&flag), "a second press exits at once");
    }

    #[test]
    fn a_second_search_in_one_process_still_finds_matches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        let plan = SearchPlan {
            roots: vec![scope::resolve_explicit(dir.path()).unwrap()],
            query: Query::new(vec![CompiledQuery::compile("a.txt".as_ref()).unwrap()]),
            visibility: VisibilityPolicy { all: false },
            exclude: Vec::new(),
            result_kind: ResultKind::Any,
            output: OutputFormat::Lines,
            order: Order::Found,
        };
        let interrupted = AtomicBool::new(false);
        for run in 1..=2 {
            let mut out = Vec::new();
            let code = execute(&plan, &mut out, &interrupted).unwrap();
            assert_eq!(
                (code, out.is_empty()),
                (status::MATCHED, false),
                "run {run}"
            );
        }
    }
}
