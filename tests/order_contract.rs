//! `-n`/`--nearest` and `-m`/`--modified`: sorted output, each root in turn.
mod common;

use std::fs::File;
use std::time::{Duration, SystemTime};

use common::{Tree, norm, zf};

/// stdout records in printed order.
fn ordered(stdout: &[u8], sep: u8) -> Vec<String> {
    stdout
        .split(|&b| b == sep)
        .filter(|r| !r.is_empty())
        .map(|r| norm(&String::from_utf8_lossy(r)))
        .collect()
}

#[test]
fn nearest_puts_shallow_results_first_then_sorts_by_name() {
    let t = Tree::with(&["b/x.txt", "a/b/x.txt", "x.txt", "a/x.txt", "c/d/e/x.txt"]);
    for flag in ["-n", "--nearest"] {
        let out = zf(t.path(), &["x.txt", flag]);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(
            ordered(&out.stdout, b'\n'),
            ["x.txt", "a/x.txt", "b/x.txt", "a/b/x.txt", "c/d/e/x.txt"],
            "{flag}"
        );
    }
}

#[test]
fn nearest_works_with_nul_output() {
    let t = Tree::with(&["a/x.txt", "x.txt"]);
    let out = zf(t.path(), &["x.txt", "-n0"]);
    assert_eq!(ordered(&out.stdout, b'\0'), ["x.txt", "a/x.txt"]);
    assert!(!out.stdout.contains(&b'\n'), "NUL-separated only");
}

#[test]
fn nearest_keeps_each_root_together_in_the_order_given() {
    let t = Tree::with(&["a/x.txt", "a/deep/er/x.txt", "b/x.txt"]);
    let out = zf(t.path(), &["x.txt", "-n", "-p", "b", "-p", "a"]);
    assert_eq!(
        ordered(&out.stdout, b'\n'),
        ["b/x.txt", "a/x.txt", "a/deep/er/x.txt"]
    );
}

#[test]
fn nearest_ignores_the_dot_slash_kept_for_dash_names() {
    // `./-x` is printed with `./` so it never reads as an option; it is still at the top.
    let t = Tree::with(&["-x", "zx", "a/x"]);
    let out = zf(t.path(), &["x", "-n"]);
    assert_eq!(ordered(&out.stdout, b'\n'), ["./-x", "zx", "a/x"]);
}

#[test]
fn sorting_with_no_matches_is_exit_1() {
    let t = Tree::with(&["a.txt"]);
    for flag in ["-n", "-m"] {
        let out = zf(t.path(), &["zzz", flag]);
        assert_eq!(
            (out.code, out.stdout.is_empty()),
            (1, true),
            "{flag}: {}",
            out.stderr
        );
    }
}

fn touch(t: &Tree, rel: &str, when: SystemTime) {
    File::options()
        .write(true)
        .open(t.join(rel))
        .unwrap()
        .set_modified(when)
        .unwrap();
}

#[test]
fn modified_puts_the_newest_first() {
    let t = Tree::with(&["old.txt", "deep/er/new.txt", "mid.txt"]);
    let ago = |s| SystemTime::now() - Duration::from_secs(s);
    touch(&t, "old.txt", ago(3000));
    touch(&t, "mid.txt", ago(2000));
    touch(&t, "deep/er/new.txt", ago(1000));
    for flag in ["-m", "--modified"] {
        let out = zf(t.path(), &[".txt", flag]);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(
            ordered(&out.stdout, b'\n'),
            ["deep/er/new.txt", "mid.txt", "old.txt"],
            "{flag}"
        );
    }
}

#[test]
fn modified_breaks_ties_nearest_first() {
    let t = Tree::with(&["b/x.txt", "x.txt", "a/x.txt"]);
    let when = SystemTime::now() - Duration::from_secs(1000);
    for rel in ["b/x.txt", "x.txt", "a/x.txt"] {
        touch(&t, rel, when);
    }
    let out = zf(t.path(), &["x.txt", "-m"]);
    assert_eq!(ordered(&out.stdout, b'\n'), ["x.txt", "a/x.txt", "b/x.txt"]);
}

#[test]
fn nearest_with_modified_is_a_usage_error() {
    let t = Tree::with(&["a.txt"]);
    assert_eq!(zf(t.path(), &["a", "-n", "-m"]).code, 2);
    assert_eq!(zf(t.path(), &["a", "-nm"]).code, 2);
}
