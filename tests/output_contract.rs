//! Streaming output and termination.

mod common;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Tree, norm, set, zf, zf_cmd};
use zerofind::domain::OutputFormat;
use zerofind::output::sink::{self, CAP, Closed, Ending, Sink};

/// Records each underlying write call, like a pipe would see them.
type WriteLog = Arc<Mutex<Vec<Vec<u8>>>>;

#[derive(Clone, Default)]
struct Probe {
    writes: WriteLog,
}

impl Write for Probe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writes.lock().unwrap().push(buf.to_vec());
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Runs the writer on `out` in a background thread, the way `lib` runs it on stdout.
fn spawn_writer<W: Write + Send + 'static>(
    slots: usize,
    out: W,
) -> (
    Arc<Sink>,
    std::thread::JoinHandle<io::Result<sink::Written>>,
) {
    let sink = Arc::new(Sink::new(slots, OutputFormat::Lines));
    let s = Arc::clone(&sink);
    let handle = std::thread::spawn(move || sink::drain(&s, out, &AtomicBool::new(false)));
    (sink, handle)
}

#[test]
fn record_is_delivered_promptly_when_the_walk_stalls() {
    let probe = Probe::default();
    let (sink, handle) = spawn_writer(1, probe.clone());
    let out = sink.producer();
    // A generous deadline: without flush-on-idle the record never arrives while the producer
    // is alive, so a tight wall-clock bound would only add flakiness on a loaded machine.
    for n in 1..=3 {
        out.push(b"a.txt").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while probe.writes.lock().unwrap().len() < n {
            assert!(
                Instant::now() < deadline,
                "record written while the producer is idle"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    sink.finish();
    handle.join().unwrap().unwrap();
}

/// Blocks every write until released, like a reader that has stopped reading.
struct Stalled(Receiver<()>, Vec<u8>);

impl Write for Stalled {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = self.0.recv();
        self.1.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_stalled_reader_bounds_buffered_output() {
    let (release, gate) = channel();
    let (sink, writer) = spawn_writer(1, Stalled(gate, Vec::new()));
    let pushed = Arc::new(AtomicUsize::new(0));
    let record = [b'x'; 99];
    let total = 20 * CAP / (record.len() + 1);
    let producer = {
        let (sink, pushed) = (Arc::clone(&sink), Arc::clone(&pushed));
        std::thread::spawn(move || {
            let out = sink.producer();
            for _ in 0..total {
                out.push(&record).unwrap();
                pushed.fetch_add(1, Ordering::Relaxed);
            }
            sink.finish();
        })
    };
    std::thread::sleep(Duration::from_millis(200));
    // A full slot, the buffer the writer swapped out, and its 32 KiB output buffer.
    let held = pushed.load(Ordering::Relaxed) * (record.len() + 1);
    let bound = 2 * (CAP + record.len()) + 32 * 1024;
    assert!(
        held <= bound,
        "{held} bytes buffered for a stalled reader (bound {bound})"
    );
    drop(release); // every later write proceeds
    producer.join().unwrap();
    let written = writer.join().unwrap().unwrap();
    assert_eq!(written.ending, Ending::Drained);
    assert_eq!(pushed.load(Ordering::Relaxed), total);
}

/// Stalls until released, then fails, like a reader that stops reading and then exits.
struct ClosesLater(Receiver<()>);

impl Write for ClosesLater {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        let _ = self.0.recv();
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        let _ = self.0.recv();
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

#[test]
fn a_closed_reader_releases_blocked_producers() {
    let (release, gate) = channel();
    let (sink, writer) = spawn_writer(1, ClosesLater(gate));
    let (done, stopped) = channel();
    {
        let sink = Arc::clone(&sink);
        std::thread::spawn(move || {
            let out = sink.producer();
            let record = [b'x'; 99];
            let _ = done.send((0..20 * CAP).find_map(|_| out.push(&record).err()));
        });
    }
    // By now the writer is stuck in a write and the producer is waiting on a full slot.
    std::thread::sleep(Duration::from_millis(200));
    drop(release);
    let stopped = stopped
        .recv_timeout(Duration::from_secs(5))
        .expect("a producer waiting on a full slot is woken when the reader closes");
    assert_eq!(stopped, Some(Closed));
    let written = writer.join().unwrap().unwrap();
    assert_eq!(written.ending, Ending::BrokenPipe);
}

/// Panics on the first write, like a writer thread that dies mid-run.
struct Panics;

impl Write for Panics {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        panic!("writer died");
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("writer died");
    }
}

#[test]
fn a_panicking_writer_releases_blocked_producers() {
    let (sink, writer) = spawn_writer(1, Panics);
    let (done, stopped) = channel();
    {
        let sink = Arc::clone(&sink);
        std::thread::spawn(move || {
            let out = sink.producer();
            let record = [b'x'; 99];
            let _ = done.send((0..20 * CAP).find_map(|_| out.push(&record).err()));
        });
    }
    let stopped = stopped
        .recv_timeout(Duration::from_secs(5))
        .expect("a producer waiting on a full slot is woken when the writer panics");
    assert_eq!(stopped, Some(Closed));
    assert!(writer.join().is_err());
}

#[test]
fn concurrent_producers_deliver_every_record_whole() {
    let probe = Probe::default();
    let (sink, writer) = spawn_writer(3, probe.clone());
    let workers: Vec<_> = (0..8)
        .map(|w| {
            let sink = Arc::clone(&sink);
            std::thread::spawn(move || {
                let out = sink.producer();
                for i in 0..5_000 {
                    out.push(format!("w{w}/record-{i:05}").as_bytes()).unwrap();
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    sink.finish();
    writer.join().unwrap().unwrap();
    let out = probe.writes.lock().unwrap().concat();
    let lines: std::collections::BTreeSet<_> = out
        .split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
        .collect();
    let expected: std::collections::BTreeSet<Vec<u8>> = (0..8)
        .flat_map(|w| (0..5_000).map(move |i| format!("w{w}/record-{i:05}").into_bytes()))
        .collect();
    assert_eq!(
        out.iter().filter(|&&b| b == b'\n').count(),
        40_000,
        "no record lost or split"
    );
    assert_eq!(
        lines
            .into_iter()
            .map(<[u8]>::to_vec)
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
}

fn drain_all(format: OutputFormat, records: &[&[u8]]) -> Vec<u8> {
    let sink = Sink::new(1, format);
    let out = sink.producer();
    for r in records {
        out.push(r).unwrap();
    }
    sink.finish();
    let mut buf = Vec::new();
    sink::drain(&sink, &mut buf, &AtomicBool::new(false)).unwrap();
    buf
}

#[test]
fn terminal_output_escapes_control_characters() {
    // A file name must not be able to drive the terminal (title, clipboard, cursor).
    let out = drain_all(
        OutputFormat::Terminal,
        &[
            b"evil\x1b]0;pwned\x07.txt",
            b"a\nb",
            "caf\u{e9}-\u{9b}x".as_bytes(),
            b"bad\xffname",
            b"src/main.rs",
        ],
    );
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "evil\\x1b]0;pwned\\x07.txt\na\\x0ab\ncaf\u{e9}-\\xc2\\x9bx\nbad\\xffname\nsrc/main.rs\n"
    );
}

#[test]
fn piped_output_stays_raw() {
    let raw: &[u8] = b"evil\x1b]0;pwned\x07.txt";
    assert_eq!(
        drain_all(OutputFormat::Lines, &[raw]),
        [raw, b"\n"].concat()
    );
}

/// Enough long-named matches to overflow a pipe buffer several times.
fn wide_tree(n: usize) -> Tree {
    let t = Tree::new();
    let dir = t.dir("bulk");
    for i in 0..n {
        std::fs::write(dir.join(format!("match-{i:05}-{}.txt", "x".repeat(60))), "").unwrap();
    }
    t
}

#[test]
fn paths_only_relative_without_dot_slash() {
    let t = Tree::with(&["src/main.rs", "examples/main.rs"]);
    let out = zf(t.path(), &["main"]);
    assert_eq!(
        String::from_utf8(out.stdout.clone())
            .unwrap()
            .lines()
            .count(),
        2,
        "no header or summary"
    );
    assert_eq!(out.lines(), set(&["src/main.rs", "examples/main.rs"]));
    assert!(out.stderr.is_empty());
}

#[test]
fn local_name_starting_with_dash_keeps_dot_slash() {
    // `zf rf | xargs rm` must not hand `rm` an option.
    let t = Tree::with(&["-rf", "x-rf.txt", "sub/-rf"]);
    assert_eq!(
        zf(t.path(), &["rf"]).lines(),
        set(&["./-rf", "x-rf.txt", "sub/-rf"])
    );
}

#[cfg(unix)] // Windows forbids newlines in file names
#[test]
fn print0_preserves_newline_in_name() {
    let t = Tree::with(&["a\nmain.txt", "b-main.txt"]);
    let out = zf(t.path(), &["main", "-0"]);
    let mut records: Vec<_> = out
        .stdout
        .split(|&b| b == 0)
        .filter(|r| !r.is_empty())
        .map(|r| String::from_utf8(r.to_vec()).unwrap())
        .collect();
    records.sort();
    assert_eq!(records, ["a\nmain.txt", "b-main.txt"]);
    assert!(out.stdout.ends_with(b"\0"));
}

#[test]
fn broken_pipe_is_quiet_success() {
    let t = wide_tree(3000);
    let (mut cmd, _home) = zf_cmd(t.path(), &["match"]);
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    assert!(norm(&first).starts_with("bulk/match-"), "{first:?}");
    let status = child.wait().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert_eq!((status.code(), stderr.as_str()), (Some(0), ""));
}

#[cfg(unix)]
#[test]
fn interrupt_emits_only_whole_paths_and_exits_130() {
    let n = 3000;
    let t = wide_tree(n);
    let (mut cmd, _home) = zf_cmd(t.path(), &["match"]);
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // The first record proves the Ctrl+C handler is installed; the unread rest keeps the
    // writer blocked on a full pipe, so the walk cannot finish first.
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut stdout = String::new();
    out.read_line(&mut stdout).unwrap();
    let kill = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill.success());
    out.read_to_string(&mut stdout).unwrap();
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(130));
    let lines: Vec<_> = stdout.lines().collect();
    assert!(lines.len() < n, "search stopped early");
    for line in lines {
        assert!(t.join(line).is_file(), "whole path record: {line:?}");
    }
}

#[cfg(unix)]
#[test]
fn closed_stderr_does_not_crash() {
    // `zf x 2>&1 | head -1`: once `head` exits, every diagnostic hits a closed pipe.
    use std::os::unix::fs::PermissionsExt;
    let t = Tree::new();
    let locked: Vec<_> = (0..50).map(|i| t.dir(&format!("locked{i}"))).collect();
    for d in &locked {
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    if std::fs::read_dir(&locked[0]).is_ok() {
        eprintln!("skipped: running with permission override (root)");
        return;
    }
    let (mut cmd, _home) = zf_cmd(t.path(), &["nomatch"]);
    let mut child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stderr.take());
    let status = child.wait().unwrap();
    for d in &locked {
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(
        status.code(),
        Some(3),
        "diagnostics are dropped; the search finishes"
    );
}

#[cfg(unix)]
#[test]
fn unreadable_directory_continues_and_exits_3() {
    use std::os::unix::fs::PermissionsExt;
    let t = Tree::with(&["locked/main.txt", "open/main.txt"]);
    let locked = t.join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&locked).is_ok() {
        eprintln!("skipped: running with permission override (root)");
        return;
    }
    let out = zf(t.path(), &["main"]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(out.lines(), set(&["open/main.txt"]));
    assert_eq!(out.code, 3);
    assert!(
        out.stderr.contains("locked: cannot read directory: "),
        "names the path and the operation: {}",
        out.stderr
    );
}

#[test]
fn repeat_gives_same_match_set() {
    let t = wide_tree(500);
    let a = zf(t.path(), &["match"]);
    let b = zf(t.path(), &["match"]);
    assert_eq!(a.lines().len(), 500);
    assert_eq!(a.lines(), b.lines());
}

// macOS (APFS) rejects file names that are not valid UTF-8.
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_name_is_emitted_raw() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let t = Tree::new();
    std::fs::write(t.path().join(OsStr::from_bytes(b"caf\xe9-main.txt")), "").unwrap();
    let out = zf(t.path(), &["main"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert_eq!(out.stdout, b"caf\xe9-main.txt\n");
}
