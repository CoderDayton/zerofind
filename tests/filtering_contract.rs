//! Visibility and pruning, through the binary.

mod common;
use common::{Tree, set, zf};

fn artifact_tree() -> Tree {
    let t = Tree::with(&[
        "src/main.rs",
        "node_modules/pkg/main.js",
        "target/debug/main",
        ".git/main",
        ".hidden/main.txt",
        ".main-env",
        "gen/main.gen",
        "notes/main-scratch.txt",
        "build/main.c",
        "venv/lib/main.py",
        "venv/pyvenv.cfg",
        "__pycache__/main.pyc",
        "Cargo.toml",
        "package.json",
    ]);
    t.file(".gitignore", "gen/\n");
    t.file(".ignore", "*-scratch.txt\n");
    t
}

#[test]
fn ignored_artifacts_are_excluded_by_default() {
    let t = artifact_tree();
    let out = zf(t.path(), &["main"]);
    assert_eq!(out.lines(), set(&["src/main.rs"]), "stderr: {}", out.stderr);
    assert_eq!(out.code, 0);
}

#[test]
fn all_includes_normally_excluded_entries() {
    let t = artifact_tree();
    let out = zf(t.path(), &["main", "--all"]);
    assert_eq!(
        out.lines(),
        set(&[
            "src/main.rs",
            "node_modules/pkg/main.js",
            "target/debug/main",
            ".git/main",
            ".hidden/main.txt",
            ".main-env",
            "gen/main.gen",
            "notes/main-scratch.txt",
            "build/main.c",
            "venv/lib/main.py",
            "__pycache__/main.pyc",
        ])
    );
}

#[test]
fn build_folders_without_a_project_are_searched() {
    let t = Tree::with(&[
        "Documents/build/notes/plan.txt",
        "Documents/dist/plan-b.txt",
    ]);
    assert_eq!(
        zf(t.path(), &["plan"]).lines(),
        set(&[
            "Documents/build/notes/plan.txt",
            "Documents/dist/plan-b.txt"
        ])
    );
}

#[test]
fn a_fragment_does_not_reach_a_skipped_directory() {
    let t = Tree::with(&["node_modules/", "target/", "Cargo.toml", "src/"]);
    assert_eq!(zf(t.path(), &["targ"]).code, 1);
    assert_eq!(zf(t.path(), &["targ", "--all"]).lines(), set(&["target"]));
}

#[test]
fn naming_a_skipped_directory_finds_it_without_entering_it() {
    let t = Tree::with(&[
        "node_modules/react/index.js",
        "node_modules/a/node_modules/b.js",
        "target/",
        "Cargo.toml",
    ]);
    assert_eq!(
        zf(t.path(), &["node_modules"]).lines(),
        set(&["node_modules"])
    );
    assert_eq!(
        zf(t.path(), &["--dirs", "TARGET"]).lines(),
        set(&["target"])
    );
    assert_eq!(
        zf(t.path(), &["node_modules/react"]).lines(),
        set(&["node_modules/react"]),
        "a path through it enters it"
    );
}

#[cfg(unix)]
#[test]
fn symlink_to_directory_is_excluded_like_one() {
    let t = Tree::with(&["out/a.txt", "root/src/", "root/package.json"]);
    std::os::unix::fs::symlink(t.join("out"), t.join("root/build")).unwrap();
    let root = t.join("root");
    assert_eq!(zf(&root, &["buil"]).code, 1);
    assert_eq!(zf(&root, &["buil", "--all"]).lines(), set(&["build"]));
}

#[test]
fn malformed_ignore_file_warns_without_partial_status() {
    // `a{b` is an unclosed alternate group; globset rejects it.
    let t = Tree::with(&["app/src/main.rs", ".git/"]);
    t.file(".gitignore", "a{b\n");
    t.file("app/src/.gitignore", "a{b\n");
    for cwd in [t.join("app"), t.path().to_path_buf()] {
        let out = zf(&cwd, &["main"]);
        assert_eq!(out.lines().len(), 1, "cwd {cwd:?}");
        assert!(out.stderr.contains(".gitignore"), "warns: {}", out.stderr);
        assert_eq!(out.code, 0, "cwd {cwd:?}: {}", out.stderr);
    }
}

#[test]
fn ignore_rules_from_parent_directories_apply() {
    let t = Tree::with(&["app/secret.key", "app/main.rs", ".git/"]);
    t.file(".gitignore", "*.key\n");
    let out = zf(&t.join("app"), &["e"]);
    assert_eq!(out.lines(), set(&[]), "parent .gitignore hides secret.key");
    assert_eq!(zf(&t.join("app"), &[".rs"]).lines(), set(&["main.rs"]));
}

#[test]
fn gitignore_outside_a_git_repository_is_not_applied() {
    // A dotfiles-style `~/.gitignore` of `*` must not hide everything below it.
    let t = Tree::with(&["Documents/notes/report.txt"]);
    t.file(".gitignore", "*\n");
    assert_eq!(
        zf(&t.join("Documents"), &["report"]).lines(),
        set(&["notes/report.txt"])
    );
    assert_eq!(
        zf(t.path(), &["report"]).lines(),
        set(&["Documents/notes/report.txt"])
    );
}

#[test]
fn gitignore_applies_inside_a_jj_repository() {
    // jj reads `.gitignore` too, so a `.jj` directory marks a repository like `.git` does.
    let t = Tree::with(&["sub/notes.txt", "sub/main.rs"]);
    t.dir(".jj");
    t.file(".gitignore", "*.txt\n");
    assert_eq!(zf(t.path(), &["n"]).lines(), set(&["sub/main.rs"]));
}

#[test]
fn relative_worktree_link_resolves_from_the_worktree() {
    // `git worktree add --relative-paths` writes a `.git` file whose gitdir is relative to the
    // worktree, not to wherever zf runs.
    let t = Tree::with(&["main/x.rs", "wt/a.tmp", "wt/b.rs"]);
    t.file("main/.git/info/exclude", "*.tmp\n");
    t.file("main/.git/worktrees/wt/commondir", "../..\n");
    t.file("wt/.git", "gitdir: ../main/.git/worktrees/wt\n");
    let wt = t.join("wt");
    let b = wt.join("b.rs");
    assert_eq!(
        zf(t.path(), &[wt.to_str().unwrap()]).lines(),
        set(&[b.to_str().unwrap()])
    );
}

#[test]
fn dot_ignore_applies_outside_a_git_repository() {
    let t = Tree::with(&["app/secret.key", "app/main.rs"]);
    t.file(".ignore", "*.key\n");
    assert_eq!(zf(&t.join("app"), &["e"]).lines(), set(&[]));
}

#[test]
fn explicitly_named_hidden_segment_is_traversed() {
    let t = Tree::with(&[".github/workflows/ci.yml", "src/main.rs"]);
    assert_eq!(
        zf(t.path(), &["./.github/workflows"]).lines(),
        set(&[".github/workflows"])
    );
    assert_eq!(
        zf(t.path(), &[".github/workflows/ci"]).lines(),
        set(&[".github/workflows/ci.yml"])
    );
    assert_eq!(
        zf(t.path(), &["workflows"]).code,
        1,
        "unnamed hidden ancestor"
    );
}

#[test]
fn a_path_through_a_skipped_directory_reaches_it() {
    let t = Tree::with(&[".git/config", ".git/hooks/pre-commit"]);
    assert_eq!(
        zf(t.path(), &["./.git/config"]).lines(),
        set(&[".git/config"])
    );
    assert_eq!(zf(t.path(), &["config"]).code, 1);
}

#[test]
fn matching_directory_is_emitted() {
    let t = Tree::with(&["src/lib.rs", "docs/"]);
    assert_eq!(zf(t.path(), &["src"]).lines(), set(&["src"]));
    assert_eq!(zf(t.path(), &["doc"]).lines(), set(&["docs"]));
}

#[test]
fn kind_filters_do_not_prune_traversal() {
    let t = Tree::with(&["main/main.txt", "src/main.rs"]);
    assert_eq!(zf(t.path(), &["main", "--dirs"]).lines(), set(&["main"]));
    assert_eq!(
        zf(t.path(), &["main", "--files"]).lines(),
        set(&["main/main.txt", "src/main.rs"])
    );
}

#[cfg(unix)]
#[test]
fn symlinked_directory_does_not_escape_root() {
    let t = Tree::with(&["outside/secret.txt", "root/inside.txt"]);
    std::os::unix::fs::symlink(t.join("outside"), t.join("root/link")).unwrap();
    let root = t.join("root");
    assert_eq!(zf(&root, &["secret"]).code, 1);
    assert_eq!(zf(&root, &["secret", "--all"]).code, 1);
    assert_eq!(
        zf(&root, &["link"]).lines(),
        set(&["link"]),
        "the link itself is an entry"
    );
}
