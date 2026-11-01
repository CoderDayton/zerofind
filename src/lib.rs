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

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;

use clap::Parser;

use cli::Cli;
use domain::policy::{ResultKind, VisibilityPolicy};
use domain::query::CompiledQuery;
use domain::scope::{self, SearchRoot};
use domain::{OutputFormat, SearchPlan};
use error::{ZfError, status};
use output::writer::{self, Ending};
use search::traversal::{self, Control};

/// Small bound on in-flight results: memory never scales with match count.
const CHANNEL_CAPACITY: usize = 1024;

/// Ctrl+C is process-wide, so its flag is too; everything else lives in one run.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub fn run() -> ExitCode {
    let cli = Cli::parse();
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
    let query = CompiledQuery::compile(&cli.query)?;
    let roots = if cli.home {
        home_roots()?
    } else if let Some(path) = &cli.path {
        vec![scope::resolve_explicit(path)?]
    } else {
        vec![SearchRoot::local()?]
    };
    Ok(SearchPlan {
        roots,
        query,
        visibility: VisibilityPolicy { all: cli.all },
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
    })
}

/// Platform-configured user folders. Conventional names under home are used only
/// when the platform configures none of them: a folder the user disabled stays disabled.
fn home_roots() -> Result<Vec<SearchRoot>, ZfError> {
    let home = dirs::home_dir();
    let configured: Vec<PathBuf> = [
        dirs::document_dir(),
        dirs::desktop_dir(),
        dirs::download_dir(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let candidates = match (&home, configured.is_empty()) {
        (Some(h), true) => ["Documents", "Desktop", "Downloads"]
            .map(|n| h.join(n))
            .to_vec(),
        _ => configured,
    };
    Ok(scope::resolve_home(&candidates, home.as_deref())?)
}

fn execute(plan: &SearchPlan, out: impl Write, interrupted: &AtomicBool) -> Result<u8, ZfError> {
    let stop = AtomicBool::new(false);
    let errors = AtomicUsize::new(0);
    let ctl = Control {
        stop: &stop,
        interrupted,
        errors: &errors,
    };
    let (tx, rx) = sync_channel(CHANNEL_CAPACITY);
    let written = std::thread::scope(|s| {
        s.spawn(|| traversal::run(plan, tx, &ctl));
        let written = writer::drain(rx, out, plan.output, interrupted);
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
            query: CompiledQuery::compile("a.txt".as_ref()).unwrap(),
            visibility: VisibilityPolicy { all: false },
            result_kind: ResultKind::Any,
            output: OutputFormat::Lines,
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
