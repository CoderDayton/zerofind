//! Command line, search scopes and errors, through the binary.

mod common;
use common::{Tree, run, set, zf, zf_cmd};

#[test]
fn help_lists_the_minimal_interface() {
    let out = zf(Tree::new().path(), &["--help"]);
    assert_eq!(out.code, 0);
    let help = String::from_utf8_lossy(&out.stdout);
    for flag in [
        "--home",
        "--path",
        "--all",
        "--files",
        "--dirs",
        "--print0",
        "--version",
    ] {
        assert!(help.contains(flag), "help mentions {flag}");
    }
}

#[test]
fn version_flag_prints_the_version() {
    let out = zf(Tree::new().path(), &["-V"]);
    assert_eq!(out.code, 0);
    assert!(String::from_utf8_lossy(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn missing_query_or_unknown_option_is_usage_error() {
    let t = Tree::new();
    for args in [
        &[][..],
        &["x", "--depth", "3"][..],
        &["x", "--files", "--dirs"][..],
    ] {
        let out = zf(t.path(), args);
        assert_eq!(out.code, 2, "args {args:?}");
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn searches_only_beneath_cwd() {
    let t = Tree::with(&["proj/main.rs", "proj/app/main.rs", "proj/app/core/main.rs"]);
    let out = zf(&t.join("proj/app"), &["main"]);
    assert_eq!(out.lines(), set(&["main.rs", "core/main.rs"]));
}

fn query_tree() -> Tree {
    Tree::with(&[
        "src/main.rs",
        "src/main.rs.bak",
        "source/main.rs",
        "packages/app/src/main.ts",
    ])
}

#[test]
fn extension_query_is_exact() {
    let out = zf(query_tree().path(), &[".rs"]);
    assert_eq!(out.lines(), set(&["src/main.rs", "source/main.rs"]));
}

#[test]
fn path_query_is_component_aware() {
    let out = zf(query_tree().path(), &["src/main"]);
    assert_eq!(
        out.lines(),
        set(&["src/main.rs", "src/main.rs.bak", "packages/app/src/main.ts"])
    );
}

#[test]
fn dot_slash_anchors_to_root() {
    let out = zf(query_tree().path(), &["./src/main"]);
    assert_eq!(out.lines(), set(&["src/main.rs", "src/main.rs.bak"]));
}

#[test]
fn root_itself_is_not_emitted() {
    let t = Tree::with(&["main/other.txt"]);
    assert_eq!(zf(&t.join("main"), &["main"]).code, 1);
}

#[test]
fn home_searches_only_configured_user_folders() {
    let home = Tree::with(&[
        "Docs/report.pdf",
        "Downloads/old-report.txt",
        "Desktop/report-desk.txt",
        "report-in-home.txt",
    ]);
    home.file(
        ".config/user-dirs.dirs",
        "XDG_DOCUMENTS_DIR=\"$HOME/Docs\"\nXDG_DESKTOP_DIR=\"$HOME/\"\nXDG_DOWNLOAD_DIR=\"$HOME/Downloads\"\n",
    );
    let (mut cmd, _isolated) = zf_cmd(home.path(), &["report", "--home"]);
    cmd.env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.join(".config"));
    let out = run(&mut cmd);
    let abs = |rel: &str| home.join(rel).to_string_lossy().into_owned();
    assert_eq!(
        out.lines(),
        [abs("Docs/report.pdf"), abs("Downloads/old-report.txt")].into(),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn home_without_user_folders_is_a_scope_error() {
    let home = Tree::with(&["report.txt"]);
    let (mut cmd, _isolated) = zf_cmd(home.path(), &["report", "--home"]);
    cmd.env("HOME", home.path());
    let out = run(&mut cmd);
    assert_eq!(out.code, 2);
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}

#[test]
fn explicit_root_is_the_boundary() {
    let t = Tree::with(&["etc/hosts", "other/hosts"]);
    let abs = t.join("etc");
    let out = zf(t.path(), &["hosts", "-p", abs.to_str().unwrap()]);
    assert_eq!(out.lines(), set(&[t.join("etc/hosts").to_str().unwrap()]));
}

#[test]
fn relative_root_keeps_its_representation() {
    let t = Tree::with(&["etc/hosts"]);
    assert_eq!(
        zf(t.path(), &["hosts", "--path", "etc"]).lines(),
        set(&["etc/hosts"])
    );
    assert_eq!(
        zf(t.path(), &["hosts", "-p", "./etc"]).lines(),
        set(&["./etc/hosts"])
    );
}

#[test]
fn explicit_file_root_is_matched_by_name() {
    let t = Tree::with(&["etc/hosts"]);
    assert_eq!(
        zf(t.path(), &["host", "-p", "etc/hosts"]).lines(),
        set(&["etc/hosts"])
    );
    assert_eq!(zf(t.path(), &["nope", "-p", "etc/hosts"]).code, 1);
}

#[test]
fn explicit_root_keeps_default_filtering() {
    let t = Tree::with(&["etc/.hidden-hosts", "etc/hosts"]);
    assert_eq!(
        zf(t.path(), &["hosts", "-p", "etc"]).lines(),
        set(&["etc/hosts"])
    );
}

#[test]
fn home_and_path_are_mutually_exclusive() {
    let t = Tree::new();
    let out = zf(t.path(), &["x", "--home", "-p", "."]);
    assert_eq!(out.code, 2);
    assert!(out.stdout.is_empty());
}

#[test]
fn inaccessible_root_is_exit_2_and_names_path() {
    let t = Tree::new();
    let out = zf(t.path(), &["x", "-p", "does-not-exist"]);
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("does-not-exist"),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn no_matches_is_quiet_exit_1() {
    let t = Tree::with(&["a.txt"]);
    let out = zf(t.path(), &["zzz"]);
    assert_eq!(
        (out.code, out.stdout.is_empty(), out.stderr.as_str()),
        (1, true, "")
    );
}

#[test]
fn parent_component_is_usage_error() {
    let t = Tree::with(&["a/b.txt"]);
    for q in ["../b", "a/../b", ".."] {
        let out = zf(&t.join("a"), &[q]);
        assert_eq!(out.code, 2, "{q}");
        assert!(out.stdout.is_empty());
        assert!(
            out.stderr.contains("--path"),
            "points to --path: {}",
            out.stderr
        );
    }
}

#[test]
fn invalid_query_diagnostic_is_brief() {
    let out = zf(Tree::new().path(), &["."]);
    assert_eq!(out.code, 2);
    assert_eq!(out.stderr.lines().count(), 1, "stderr: {}", out.stderr);
}
