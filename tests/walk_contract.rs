//! The walk adds workers only while there is more work than idle workers, and reports itself
//! on stderr when `ZF_TRACE=1`.
mod common;

use common::{Out, Tree, run, zf, zf_cmd};

fn traced(cwd: &std::path::Path) -> Out {
    let (mut cmd, _home) = zf_cmd(cwd, &[]);
    cmd.env("ZF_TRACE", "1");
    run(&mut cmd)
}

/// The value of `key=` in the trace line.
fn field(out: &Out, key: &str) -> usize {
    let line = out
        .stderr
        .lines()
        .find(|l| l.starts_with("zf: trace: "))
        .expect("a trace line");
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("no {key}= in {line:?}"))
        .parse()
        .unwrap()
}

#[test]
fn trace_prints_one_line_per_directory_root() {
    let t = Tree::with(&["a/x", "b/x", "f"]);
    let (mut cmd, _home) = zf_cmd(t.path(), &["x", "-p", "a", "-p", "b", "-p", "f"]);
    cmd.env("ZF_TRACE", "1");
    let out = run(&mut cmd);
    let roots: Vec<_> = out
        .stderr
        .lines()
        .map(|l| l.split(": ").nth(2).unwrap_or(l))
        .collect();
    assert_eq!(
        roots,
        ["a", "b"],
        "a file root is not walked: {}",
        out.stderr
    );
}

#[test]
fn trace_reports_the_walk_on_stderr_only() {
    let t = Tree::with(&["a/x.rs", "a/b/y.rs", "c.rs"]);
    let plain = zf(t.path(), &[]);
    let out = traced(t.path());
    assert_eq!(out.lines(), plain.lines(), "tracing never changes results");
    assert_eq!(out.code, 0);
    assert_eq!(
        out.stderr.lines().count(),
        1,
        "one line per root: {}",
        out.stderr
    );
    assert_eq!(field(&out, "dirs"), 3);
    assert_eq!(field(&out, "entries"), 5);
    assert!(plain.stderr.is_empty(), "no trace without the variable");
}

#[test]
fn a_chain_of_directories_needs_one_worker() {
    let t = Tree::new();
    t.file(&format!("{}f", "d/".repeat(200)), "");
    let out = traced(t.path());
    assert_eq!(field(&out, "dirs"), 201);
    assert_eq!(field(&out, "workers"), 1);
}

#[test]
fn a_wide_tree_adds_workers() {
    if std::thread::available_parallelism().map_or(1, |n| n.get()) < 2 {
        return;
    }
    let t = Tree::new();
    for i in 0..64 {
        for j in 0..8 {
            t.file(&format!("d{i}/e{j}/f"), "");
        }
    }
    let out = traced(t.path());
    assert_eq!(field(&out, "dirs"), 1 + 64 + 64 * 8);
    assert!(field(&out, "workers") > 1, "{}", out.stderr);
}
