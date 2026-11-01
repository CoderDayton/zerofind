//! Streaming output and termination.

mod common;
use std::io::{BufRead, BufReader, Read};
use std::process::Stdio;

use common::{Tree, set, zf, zf_cmd};

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
