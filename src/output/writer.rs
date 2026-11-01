//! The single writer: streams records as they arrive, flushing whenever the queue runs dry.

use std::io::{self, BufWriter, Write};
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

/// Records are raw path bytes; non-UTF-8 names pass through unchanged.
pub fn drain<R: AsRef<[u8]>, W: Write>(
    rx: Receiver<R>,
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

fn write_all<R: AsRef<[u8]>, W: Write>(
    rx: &Receiver<R>,
    out: W,
    format: OutputFormat,
    interrupted: &AtomicBool,
    any: &mut bool,
) -> io::Result<Ending> {
    let sep = match format {
        OutputFormat::Lines | OutputFormat::Terminal => b'\n',
        OutputFormat::Nul => b'\0',
    };
    let mut out = BufWriter::with_capacity(32 * 1024, out);
    loop {
        let record = match rx.try_recv() {
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
        match format {
            OutputFormat::Terminal => write_escaped(&mut out, record.as_ref())?,
            OutputFormat::Lines | OutputFormat::Nul => out.write_all(record.as_ref())?,
        }
        out.write_all(&[sep])?;
        *any = true;
    }
    out.flush()?;
    Ok(Ending::Drained)
}

/// Writes `bytes` as valid UTF-8 without control characters: those and invalid bytes become
/// `\xNN`, so a file name cannot drive the terminal.
fn write_escaped(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    for chunk in bytes.utf8_chunks() {
        let valid = chunk.valid();
        let mut start = 0;
        for (i, c) in valid.char_indices() {
            if c.is_control() {
                let end = i + c.len_utf8();
                out.write_all(&valid.as_bytes()[start..i])?;
                hex(out, &valid.as_bytes()[i..end])?;
                start = end;
            }
        }
        out.write_all(&valid.as_bytes()[start..])?;
        hex(out, chunk.invalid())?;
    }
    Ok(())
}

fn hex(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    bytes.iter().try_for_each(|b| write!(out, "\\x{b:02x}"))
}
