//! Streaming output and termination.

mod common;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Tree, set, zf, zf_cmd};
use zerofind::domain::OutputFormat;
use zerofind::output::writer;

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

/// Runs the writer on a pipe-like (non-terminal) output in a background thread.
fn pipe_writer(
    probe: &Probe,
) -> (
    std::sync::mpsc::SyncSender<String>,
    std::thread::JoinHandle<()>,
) {
    let (tx, rx) = sync_channel(1024);
    let out = probe.clone();
    let handle = std::thread::spawn(move || {
        let stop = AtomicBool::new(false);
        writer::drain(rx, out, OutputFormat::Lines, &stop).unwrap();
    });
    (tx, handle)
}

#[test]
fn record_is_delivered_promptly_when_the_walk_stalls() {
    let probe = Probe::default();
    let (tx, handle) = pipe_writer(&probe);
    // A generous deadline: without flush-on-idle the record never arrives while the sender
    // is alive, so a tight wall-clock bound would only add flakiness on a loaded machine.
    for n in 1..=3 {
        tx.send("a.txt".to_owned()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while probe.writes.lock().unwrap().len() < n {
            assert!(
                Instant::now() < deadline,
                "record written while the sender is idle"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    drop(tx);
    handle.join().unwrap();
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
    assert!(first.starts_with("bulk/match-"));
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
    // The writer is now blocked on a full pipe.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let kill = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(kill.success());
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
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
        out.stderr.contains("locked"),
        "names the path: {}",
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

#[cfg(unix)]
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
