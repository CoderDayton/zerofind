//! Shared harness: temp trees built in code, and an isolated `zf` runner.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;

pub struct Tree {
    dir: TempDir,
}

impl Tree {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    /// Creates each path; a trailing `/` makes a directory, anything else an empty file.
    pub fn with(entries: &[&str]) -> Self {
        let tree = Self::new();
        for e in entries {
            if let Some(d) = e.strip_suffix('/') {
                tree.dir(d);
            } else {
                tree.file(e, "");
            }
        }
        tree
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.path().join(rel)
    }

    pub fn dir(&self, rel: &str) -> PathBuf {
        let p = self.join(rel);
        fs::create_dir_all(&p).unwrap();
        p
    }

    pub fn file(&self, rel: &str, content: &str) -> PathBuf {
        let p = self.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }
}

pub struct Out {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Out {
    /// Newline-separated records as a sorted set: result order is unspecified.
    pub fn lines(&self) -> BTreeSet<String> {
        String::from_utf8_lossy(&self.stdout)
            .lines()
            .map(norm)
            .collect()
    }
}

pub fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| norm(s)).collect()
}

/// Windows prints native `\` separators; expectations are written with `/`.
fn norm(path: &str) -> String {
    if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    }
}

/// A `zf` command in `cwd` with a private, empty HOME so user-level git ignore rules and
/// folder settings never leak into a test.
pub fn zf_cmd(cwd: &Path, args: &[&str]) -> (Command, TempDir) {
    let home = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zf"));
    cmd.current_dir(cwd)
        .args(args)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env_remove("GIT_DIR")
        .stdin(Stdio::null());
    (cmd, home)
}

pub fn zf(cwd: &Path, args: &[&str]) -> Out {
    let (mut cmd, _home) = zf_cmd(cwd, args);
    run(&mut cmd)
}

pub fn run(cmd: &mut Command) -> Out {
    let out = cmd.output().unwrap();
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}
