//! Per-worker output buffers drained by the single writer.
//!
//! Each worker formats its matches into its own buffer. The writer swaps a filled buffer for an
//! empty one and writes it whole, so a match costs one uncontended lock and a copy instead of a
//! trip through a shared queue. Buffers are swapped, never freed across threads.

use std::io::{self, BufWriter, Write};
use std::mem;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use crate::domain::OutputFormat;

/// A worker waits for the writer once its buffer holds this much, so output memory is bounded
/// by slots + 1 buffers (one spare for swapping) of under 2 x CAP capacity each, whatever the
/// match count or reader speed.
pub const CAP: usize = 16 * 1024;

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

/// The writer stopped (closed pipe, write error or Ctrl+C); producers should stop too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

pub struct Sink {
    slots: Box<[Slot]>,
    next: AtomicUsize,
    format: OutputFormat,
    wake: Signal,
    done: AtomicBool,
    closed: AtomicBool,
}

/// A wake-up for the writer that is never lost: it stays raised until the writer takes it.
/// (`thread::park` tokens are not enough: any library code on the writer thread that parks,
/// such as a channel receive, can consume one.) Raising is one atomic swap unless the writer
/// is asleep, so a busy writer costs producers no futex calls.
struct Signal {
    raised: AtomicBool,
    sleeping: AtomicBool,
    lock: Mutex<()>,
    cv: Condvar,
}

impl Signal {
    fn new() -> Self {
        Self {
            raised: AtomicBool::new(false),
            sleeping: AtomicBool::new(false),
            lock: Mutex::new(()),
            cv: Condvar::new(),
        }
    }

    fn raise(&self) {
        // SeqCst pairs with `wait`: either it sees `raised`, or this sees `sleeping`.
        if !self.raised.swap(true, Ordering::SeqCst) && self.sleeping.load(Ordering::SeqCst) {
            // Notify under the lock, so a writer between its check and its wait still wakes.
            let _held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
            self.cv.notify_one();
        }
    }

    fn wait(&self) {
        if self.raised.swap(false, Ordering::SeqCst) {
            return;
        }
        let mut held = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.sleeping.store(true, Ordering::SeqCst);
        while !self.raised.swap(false, Ordering::SeqCst) {
            held = self.cv.wait(held).unwrap_or_else(PoisonError::into_inner);
        }
        self.sleeping.store(false, Ordering::SeqCst);
    }
}

struct Slot {
    buf: Mutex<Vec<u8>>,
    room: Condvar,
}

fn lock(slot: &Slot) -> MutexGuard<'_, Vec<u8>> {
    slot.buf.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Sink {
    pub fn new(slots: usize, format: OutputFormat) -> Self {
        Self {
            slots: (0..slots.max(1))
                .map(|_| Slot {
                    buf: Mutex::new(Vec::new()),
                    room: Condvar::new(),
                })
                .collect(),
            next: AtomicUsize::new(0),
            format,
            wake: Signal::new(),
            done: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        }
    }

    /// A producer on the next slot. Producers may share a slot; they then take turns.
    pub fn producer(&self) -> Producer<'_> {
        let i = self.next.fetch_add(1, Ordering::Relaxed) % self.slots.len();
        Producer {
            sink: self,
            slot: &self.slots[i],
        }
    }

    /// No more records will be pushed. Call it even when producers fail, or `drain` never ends:
    /// `Finish` does that on drop.
    pub fn finish(&self) {
        self.done.store(true, Ordering::Release);
        self.wake.raise();
    }

    /// Marks the writer gone and wakes producers blocked on a full slot.
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        for slot in self.slots.iter() {
            // Notify under the lock, so a producer between its check and its wait still wakes.
            let _held = lock(slot);
            slot.room.notify_all();
        }
    }
}

/// Calls [`Sink::finish`] when dropped, so a panicking walk still lets the writer finish.
pub struct Finish<'a>(pub &'a Sink);

impl Drop for Finish<'_> {
    fn drop(&mut self) {
        self.0.finish();
    }
}

pub struct Producer<'a> {
    sink: &'a Sink,
    slot: &'a Slot,
}

impl Producer<'_> {
    /// Appends one record (raw path bytes) in the sink's output format.
    pub fn push(&self, path: &[u8]) -> Result<(), Closed> {
        let mut buf = lock(self.slot);
        while buf.len() >= CAP && !self.sink.closed.load(Ordering::Acquire) {
            buf = self
                .slot
                .room
                .wait(buf)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if self.sink.closed.load(Ordering::Acquire) {
            return Err(Closed);
        }
        let was_empty = buf.is_empty();
        encode(&mut buf, path, self.sink.format);
        drop(buf);
        if was_empty {
            self.sink.wake.raise();
        }
        Ok(())
    }
}

/// Appends one record in `format`, terminator included.
pub fn encode(buf: &mut Vec<u8>, path: &[u8], format: OutputFormat) {
    match format {
        OutputFormat::Terminal => escape_into(buf, path),
        OutputFormat::Lines | OutputFormat::Nul => buf.extend_from_slice(path),
    }
    buf.push(match format {
        OutputFormat::Lines | OutputFormat::Terminal => b'\n',
        OutputFormat::Nul => b'\0',
    });
}

/// Drains the sink into `out` on the calling thread until every producer has finished, the
/// reader closes the pipe, or Ctrl+C. Only whole records are ever written.
pub fn drain<W: Write>(sink: &Sink, out: W, interrupted: &AtomicBool) -> io::Result<Written> {
    // Closes on drop, so a panicking writer still releases producers waiting on a full slot.
    struct Close<'a>(&'a Sink);
    impl Drop for Close<'_> {
        fn drop(&mut self) {
            self.0.close();
        }
    }
    let close = Close(sink);
    let mut any = false;
    let result = write_all(sink, out, interrupted, &mut any);
    drop(close);
    match result {
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
    sink: &Sink,
    out: W,
    interrupted: &AtomicBool,
    any: &mut bool,
) -> io::Result<Ending> {
    let mut out = BufWriter::with_capacity(32 * 1024, out);
    let mut spare = Vec::new();
    loop {
        let mut wrote = false;
        for slot in sink.slots.iter() {
            {
                let mut buf = lock(slot);
                if buf.is_empty() {
                    continue;
                }
                mem::swap(&mut *buf, &mut spare);
            }
            slot.room.notify_all();
            if interrupted.load(Ordering::Relaxed) {
                out.flush()?;
                return Ok(Ending::Interrupted);
            }
            out.write_all(&spare)?;
            spare.clear();
            *any = true;
            wrote = true;
        }
        if wrote {
            continue;
        }
        // Nothing queued: a lone match must not wait in the buffer.
        out.flush()?;
        if sink.done.load(Ordering::Acquire) {
            // Producers finished before this pass, so an empty pass means everything is out.
            if sink.slots.iter().all(|s| lock(s).is_empty()) {
                return Ok(Ending::Drained);
            }
            continue;
        }
        sink.wake.wait();
    }
}

/// Appends `bytes` as valid UTF-8 without control characters: those and invalid bytes become
/// `\xNN`, so a file name cannot drive the terminal.
fn escape_into(out: &mut Vec<u8>, bytes: &[u8]) {
    for chunk in bytes.utf8_chunks() {
        let valid = chunk.valid();
        let mut start = 0;
        for (i, c) in valid.char_indices() {
            if c.is_control() {
                let end = i + c.len_utf8();
                out.extend_from_slice(&valid.as_bytes()[start..i]);
                hex(out, &valid.as_bytes()[i..end]);
                start = end;
            }
        }
        out.extend_from_slice(&valid.as_bytes()[start..]);
        hex(out, chunk.invalid());
    }
}

fn hex(out: &mut Vec<u8>, bytes: &[u8]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for &b in bytes {
        out.extend_from_slice(&[
            b'\\',
            b'x',
            DIGITS[(b >> 4) as usize],
            DIGITS[(b & 15) as usize],
        ]);
    }
}
