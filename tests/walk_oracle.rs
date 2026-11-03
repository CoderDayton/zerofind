//! zf's walker against the `ignore` crate's, on generated trees with random ignore rules.
//!
//! zf applies ignore files itself so it can add workers mid-walk; this pins its rule handling
//! (precedence, repository boundaries, rules above the root) to the crate it replaced.
mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use common::{Tree, zf};

/// What zf's walk must yield: the crate's walker configured as zf configured it, minus
/// hidden entries (zf skips them when no query names them).
fn oracle(root: &Path) -> BTreeSet<String> {
    listing(root, true)
}

fn listing(root: &Path, rules: bool) -> BTreeSet<String> {
    let mut b = ignore::WalkBuilder::new(root);
    if rules {
        b.add_custom_ignore_filename(".fdignore");
    }
    b.standard_filters(false)
        .hidden(false)
        .parents(rules)
        .ignore(rules)
        .git_ignore(rules)
        // zf runs with an empty private HOME, so there is no global file to read.
        .git_global(false)
        .git_exclude(rules)
        .require_git(true)
        .follow_links(false)
        .filter_entry(|e| e.depth() == 0 || !e.file_name().as_encoded_bytes().starts_with(b"."));
    b.build()
        .filter_map(Result::ok)
        .filter(|e| e.depth() > 0)
        .map(|e| {
            let rel = e.path().strip_prefix(root).unwrap();
            rel.to_string_lossy().replace('\\', "/")
        })
        .collect()
}

/// Asserts zf matches the crate; true if rules removed anything, so the case tested them.
fn assert_same(root: &Path, context: &str) -> bool {
    let want = oracle(root);
    let out = zf(root, &[]);
    assert_eq!(out.lines(), want, "{context}\nstderr: {}", out.stderr);
    want != listing(root, false)
}

/// xorshift64: deterministic and dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }
}

const DIRS: &[&str] = &["a", "b", "c", "lib", "doc", "Ä", "sp ace"];
const FILES: &[&str] = &[
    "x.log", "y.txt", "z.rs", "keep.log", "a", "!x", "Ä.txt", "main.rs",
];
const RULES: &[&str] = &[
    "*.log",
    "!keep.log",
    "/a",
    "b/",
    "c",
    "**/doc",
    "lib/*.rs",
    "*.txt",
    "a/**/z.rs",
    "\\!x",
    "doc/  ",
    "# comment",
    "",
    "*",
    "!*/",
    "!z.rs",
    "x.*",
    "[ab]",
    "y.txt/",
    "/lib/",
    "Ä*",
    "sp\\ ace",
    "!/c",
    "**",
    "a/",
];

fn rules(rng: &mut Rng) -> String {
    (0..1 + rng.below(4))
        .map(|_| format!("{}\n", rng.pick(RULES)))
        .collect()
}

fn grow(t: &Tree, rng: &mut Rng, dir: &str, depth: u32) {
    for _ in 0..rng.below(4) {
        let file = format!("{dir}{}", rng.pick(FILES));
        if !t.join(&file).exists() {
            t.file(&file, "");
        }
    }
    match rng.below(10) {
        0 => {
            t.dir(&format!("{dir}.git"));
            if rng.below(2) == 0 {
                t.file(&format!("{dir}.git/info/exclude"), &rules(rng));
            }
        }
        1 => {
            t.dir(&format!("{dir}.jj"));
        }
        _ => {}
    }
    if rng.below(3) == 0 {
        t.file(&format!("{dir}.gitignore"), &rules(rng));
    }
    if rng.below(5) == 0 {
        t.file(&format!("{dir}.ignore"), &rules(rng));
    }
    if rng.below(6) == 0 {
        t.file(&format!("{dir}.fdignore"), &rules(rng));
    }
    if depth < 4 {
        for _ in 0..rng.below(4) {
            let name = rng.pick(DIRS);
            if t.join(&format!("{dir}{name}")).is_file() {
                continue;
            }
            let sub = format!("{dir}{name}/");
            t.dir(&sub);
            grow(t, rng, &sub, depth + 1);
        }
    }
}

#[test]
fn random_trees_match_the_ignore_crate() {
    let mut exercised = 0;
    for seed in 1..=200u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let t = Tree::new();
        grow(&t, &mut rng, "", 0);
        exercised += usize::from(assert_same(t.path(), &format!("seed {seed}, whole tree")));
        // A subdirectory as the root: rules above it still apply.
        for sub in DIRS {
            if t.join(sub).is_dir() {
                assert_same(&t.join(sub), &format!("seed {seed}, root {sub}"));
                break;
            }
        }
    }
    assert!(
        exercised >= 80,
        "only {exercised} of 200 trees had a rule remove anything"
    );
}

#[test]
fn nested_repositories_stop_git_rules_at_their_boundary() {
    let t = Tree::with(&["in/x.log", "in/inner/x.log", "in/inner/y.txt", "out.log"]);
    t.dir(".git");
    t.file(".gitignore", "*.log\n");
    t.dir("in/inner/.git");
    t.file("in/inner/.gitignore", "*.txt\n");
    t.file(".ignore", "out*\n");
    assert_same(t.path(), "nested repositories");
    assert_same(&t.join("in"), "root inside the outer repository");
}

#[test]
fn worktree_git_file_reads_the_common_exclude() {
    let t = Tree::with(&["main/a.tmp", "wt/a.tmp", "wt/b.rs"]);
    t.file("main/.git/info/exclude", "*.tmp\n");
    t.file("main/.git/worktrees/wt/commondir", "../..\n");
    let gitdir = t.join("main/.git/worktrees/wt");
    t.file("wt/.git", &format!("gitdir: {}\n", gitdir.display()));
    assert_same(&t.join("wt"), "worktree");
}

#[test]
fn deep_tree_with_rules_at_every_level() {
    let t = Tree::new();
    t.dir(".git");
    let mut dir = String::new();
    for i in 0..60 {
        dir.push_str(&format!("d{i}/"));
        t.file(&format!("{dir}.gitignore"), &format!("f{}\n!keep\n", i % 7));
        t.file(&format!("{dir}f{}", i % 5), "");
        t.file(&format!("{dir}keep"), "");
    }
    assert_same(t.path(), "deep");
}

#[cfg(unix)]
#[test]
fn symlinks_are_listed_never_entered() {
    let t = Tree::with(&["real/x.log", "real/y.txt"]);
    t.file(".ignore", "y.txt\n");
    std::os::unix::fs::symlink(t.join("real"), t.join("link")).unwrap();
    std::os::unix::fs::symlink(t.join("real/x.log"), t.join("file-link")).unwrap();
    std::os::unix::fs::symlink(t.join("missing"), t.join("dangling")).unwrap();
    assert_same(t.path(), "symlinks");
}

#[test]
fn ignore_file_that_is_a_directory_is_skipped() {
    let t = Tree::with(&["x.log", "y.txt"]);
    t.dir(".git");
    t.dir(".gitignore");
    t.file(".ignore", "*.log\n");
    assert_same(t.path(), "directory named .gitignore");
    fs::remove_dir(t.join(".gitignore")).unwrap();
}
