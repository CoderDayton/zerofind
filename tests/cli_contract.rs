//! Command line, search scopes and errors, through the binary.

mod common;
use common::{Tree, set, zf};
#[cfg(unix)]
use common::{run, zf_cmd};

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
fn unknown_option_is_usage_error() {
    let t = Tree::new();
    for args in [&["x", "--depth", "3"][..], &["x", "--files", "--dirs"][..]] {
        let out = zf(t.path(), args);
        assert_eq!(out.code, 2, "args {args:?}");
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn no_query_lists_everything_visible() {
    let t = Tree::with(&["a.txt", "src/b.rs", ".hidden", "node_modules/x.js"]);
    for args in [&[][..], &["."][..], &["./"][..]] {
        assert_eq!(
            zf(t.path(), args).lines(),
            set(&["a.txt", "src", "src/b.rs"]),
            "args {args:?}"
        );
    }
}

#[test]
fn every_word_must_match() {
    let t = Tree::with(&[
        "annual report.txt",
        "2024-Annual-Report.pdf",
        "annual budget.txt",
    ]);
    assert_eq!(
        zf(t.path(), &["annual", "report"]).lines(),
        set(&["annual report.txt", "2024-Annual-Report.pdf"])
    );
    assert_eq!(
        zf(t.path(), &["report", ".pdf"]).lines(),
        set(&["2024-Annual-Report.pdf"])
    );
}

#[test]
fn glob_query_matches_whole_names() {
    let t = Tree::with(&["src/main.rs", "src/main.rs.bak", "notes.txt"]);
    assert_eq!(zf(t.path(), &["*.rs"]).lines(), set(&["src/main.rs"]));
}

#[test]
fn query_starting_with_dash_needs_no_separator() {
    let t = Tree::with(&["-draft.md", "final.md", "-2024.log"]);
    assert_eq!(zf(t.path(), &["-draft"]).lines(), set(&["./-draft.md"]));
    assert_eq!(zf(t.path(), &["-2024"]).lines(), set(&["./-2024.log"]));
    assert_eq!(
        zf(t.path(), &["--", "-draft"]).lines(),
        set(&["./-draft.md"])
    );
    assert_eq!(zf(t.path(), &["-0", "final"]).stdout, b"final.md\0");
}

#[test]
fn dotfile_is_found_by_its_name_anywhere() {
    let t = Tree::with(&[
        ".env",
        "app/.env",
        "api/prod.env",
        "api/.envrc",
        ".config/x/.env",
    ]);
    assert_eq!(
        zf(t.path(), &[".env"]).lines(),
        set(&[".env", "app/.env", "api/prod.env"]),
        "a named hidden directory is not entered"
    );
}

#[test]
fn absolute_location_becomes_the_root() {
    let t = Tree::with(&[
        "Downloads/report.pdf",
        "Downloads/old/report-1.txt",
        "other/report.txt",
    ]);
    let dl = t.join("Downloads");
    let abs = |rel: &str| t.join(rel).to_string_lossy().into_owned();
    let both = [
        abs("Downloads/report.pdf"),
        abs("Downloads/old/report-1.txt"),
    ];
    let joined = dl.join("report");
    assert_eq!(
        zf(t.path(), &[joined.to_str().unwrap()]).lines(),
        both.clone().into()
    );
    assert_eq!(
        zf(t.path(), &["report", dl.to_str().unwrap()]).lines(),
        both.into()
    );
    let listed = zf(t.path(), &[dl.to_str().unwrap()]).lines();
    assert!(
        listed.contains(&abs("Downloads/old")),
        "a directory alone lists it"
    );
}

// Windows resolves the home directory through the shell API, not HOME.
#[cfg(unix)]
#[test]
fn parent_and_tilde_locations() {
    let t = Tree::with(&["a/b.txt", "c/d.txt"]);
    assert_eq!(zf(&t.join("a"), &["../c/d"]).lines(), set(&["../c/d.txt"]));
    assert_eq!(zf(&t.join("a"), &["..", "d"]).lines(), set(&["../c/d.txt"]));
    let (mut cmd, _isolated) = zf_cmd(t.path(), &["~/c/d"]);
    cmd.env("HOME", t.path());
    let out = run(&mut cmd);
    assert_eq!(
        out.lines(),
        [t.join("c/d.txt").to_string_lossy().into_owned()].into(),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn location_with_a_missing_directory_is_exit_2() {
    let t = Tree::new();
    let missing = t.join("nope/deeper/report");
    let out = zf(t.path(), &[missing.to_str().unwrap()]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("nope"), "stderr: {}", out.stderr);
}

#[test]
fn location_naming_a_file_evaluates_it() {
    let t = Tree::with(&["etc/hosts"]);
    let f = t.join("etc/hosts");
    assert_eq!(
        zf(t.path(), &[f.to_str().unwrap()]).lines(),
        set(&[f.to_str().unwrap()])
    );
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

// Windows resolves the home directory through the shell API, not HOME.
#[cfg(unix)]
#[test]
fn home_searches_the_whole_home_directory() {
    let home = Tree::with(&[
        "Documents/report.pdf",
        "projects/app/report.rs",
        "report-in-home.txt",
        ".cache/report.bin",
    ]);
    let cwd = Tree::new();
    let (mut cmd, _isolated) = zf_cmd(cwd.path(), &["report", "--home"]);
    cmd.env("HOME", home.path());
    let out = run(&mut cmd);
    let abs = |rel: &str| home.join(rel).to_string_lossy().into_owned();
    assert_eq!(
        out.lines(),
        [
            abs("Documents/report.pdf"),
            abs("projects/app/report.rs"),
            abs("report-in-home.txt"),
        ]
        .into(),
        "stderr: {}",
        out.stderr
    );
}

#[test]
fn overlapping_roots_report_each_entry_once() {
    let t = Tree::with(&["a/b/x.txt"]);
    let a = t.join("a");
    let b = t.join("a/b");
    let out = zf(
        t.path(),
        &[
            "x",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            a.to_str().unwrap(),
        ],
    );
    assert_eq!(
        out.stdout.iter().filter(|&&c| c == b'\n').count(),
        1,
        "{:?}",
        out.lines()
    );
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
fn invalid_query_diagnostic_is_brief() {
    let out = zf(Tree::new().path(), &[""]);
    assert_eq!(out.code, 2);
    assert_eq!(out.stderr.lines().count(), 1, "stderr: {}", out.stderr);
}
