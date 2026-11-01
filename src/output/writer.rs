//! The single writer: streams records as they arrive, flushing whenever the queue runs dry.

use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use crate::domain::OutputFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Drained,
    BrokenPipe,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    pub any: bool,
    pub ending: Ending,
}

pub fn drain<W: Write>(
    rx: Receiver<PathBuf>,
    out: W,
    format: OutputFormat,
    interrupted: &AtomicBool,
) -> io::Result<Written> {
    let mut any = false;
    match write_all(&rx, out, format, interrupted, &mut any) {
        Ok(ending) => Ok(Written { any, ending }),
        // Downstream closed the pipe: a quiet success.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(Written {
            any,
            ending: Ending::BrokenPipe,
        }),
        Err(e) => Err(e),
    }
}

fn write_all<W: Write>(
    rx: &Receiver<PathBuf>,
    out: W,
    format: OutputFormat,
    interrupted: &AtomicBool,
    any: &mut bool,
) -> io::Result<Ending> {
    let sep = match format {
        OutputFormat::Lines => b'\n',
        OutputFormat::Nul => b'\0',
    };
    let mut out = BufWriter::with_capacity(32 * 1024, out);
    loop {
        let path = match rx.try_recv() {
            Ok(p) => p,
            Err(TryRecvError::Empty) => {
                out.flush()?;
                match rx.recv() {
                    Ok(p) => p,
                    Err(_) => break,
                }
            }
            Err(TryRecvError::Disconnected) => break,
        };
        // Only whole records ever reach stdout, so stopping here is clean.
        if interrupted.load(Ordering::Relaxed) {
            out.flush()?;
            return Ok(Ending::Interrupted);
        }
        out.write_all(path.as_os_str().as_encoded_bytes())?;
        out.write_all(&[sep])?;
        *any = true;
    }
    out.flush()?;
    Ok(Ending::Drained)
}
