//! ZeroFind: stream-first filesystem discovery.

pub mod cli;
pub mod domain;
pub mod error;
pub mod output;
pub mod search;

use std::io;
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

static STOP: AtomicBool = AtomicBool::new(false);
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    match plan(&cli).and_then(|plan| execute(&plan)) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("zf: {e}");
            ExitCode::from(e.status())
        }
    }
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

fn execute(plan: &SearchPlan) -> Result<u8, ZfError> {
    // Without a handler, Ctrl+C still ends the process; it just skips the clean exit.
    let _ = ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::Relaxed);
        STOP.store(true, Ordering::Relaxed);
    });
    let errors = AtomicUsize::new(0);
    let ctl = Control {
        stop: &STOP,
        errors: &errors,
    };
    let (tx, rx) = sync_channel(CHANNEL_CAPACITY);
    let written = std::thread::scope(|s| {
        s.spawn(|| traversal::run(plan, tx, &ctl));
        let written = writer::drain(rx, io::stdout().lock(), plan.output, &INTERRUPTED);
        STOP.store(true, Ordering::Relaxed);
        written
    })
    .map_err(ZfError::Output)?;

    Ok(match written.ending {
        Ending::BrokenPipe => status::MATCHED,
        _ if INTERRUPTED.load(Ordering::Relaxed) => status::INTERRUPTED,
        _ if errors.load(Ordering::Relaxed) > 0 => status::PARTIAL,
        _ if written.any => status::MATCHED,
        _ => status::NO_MATCH,
    })
}
