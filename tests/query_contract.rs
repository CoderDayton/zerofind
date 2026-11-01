//! Query interpretation and matching, against the library API.

use std::ffi::OsStr;
use std::path::Path;

use zerofind::domain::query::{CompiledQuery, QueryError, QueryMode};

fn q(raw: &str) -> CompiledQuery {
    CompiledQuery::compile(OsStr::new(raw)).expect("query compiles")
}

fn err(raw: &str) -> QueryError {
    CompiledQuery::compile(OsStr::new(raw)).expect_err("query is rejected")
}

/// Matches a file entry at the given root-relative path.
fn file(query: &str, rel: &str) -> bool {
    q(query).matches(Path::new(rel), false)
}

fn dir(query: &str, rel: &str) -> bool {
    q(query).matches(Path::new(rel), true)
}

#[test]
fn only_dir_queries_need_the_entry_kind() {
    assert!(q("src/").needs_kind());
    for raw in ["src", ".rs", "src/main", "./src"] {
        assert!(!q(raw).needs_kind(), "{raw}");
    }
}

#[test]
fn separator_selects_path_mode() {
    assert_eq!(q("src/main").mode(), QueryMode::Path);
    assert_eq!(q("a/.rs").mode(), QueryMode::Path);
    assert_eq!(q("./main").mode(), QueryMode::Path);
}

#[test]
fn single_leading_dot_selects_extension_mode() {
    assert_eq!(q(".rs").mode(), QueryMode::Extension);
    assert_eq!(q(".config").mode(), QueryMode::Extension);
}

#[test]
fn everything_else_is_name_mode() {
    assert_eq!(q("index").mode(), QueryMode::Name);
    assert_eq!(q(".tar.gz").mode(), QueryMode::Name);
    assert_eq!(q("main.rs").mode(), QueryMode::Name);
}

#[test]
fn degenerate_queries_are_rejected() {
    assert_eq!(err(""), QueryError::Empty);
    assert_eq!(err("."), QueryError::EmptyExtension);
    assert_eq!(err("/etc/hosts"), QueryError::Absolute);
    assert_eq!(err("./"), QueryError::NoComponents);
}

#[test]
fn basename_substring_case_insensitive() {
    for (query, yes, no) in [
        ("index", ["index.ts", "my-index.js"], "main.rs"),
        (
            "config",
            ["config.json", "app-config.toml"],
            "settings.yaml",
        ),
        ("README", ["README.md", "readme.txt"], "manual.md"),
        ("main.rs", ["main.rs", "old-main.rs"], "main.ts"),
    ] {
        for name in yes {
            assert!(file(query, name), "{query} should match {name}");
        }
        assert!(!file(query, no), "{query} should not match {no}");
    }
}

#[test]
fn ancestors_do_not_match() {
    assert!(!file("index", "index/main.rs"));
    assert!(file("index", "src/index.ts"));
}

#[test]
fn exact_final_extension() {
    assert!(file(".rs", "main.rs"));
    assert!(file(".rs", "src/lib.RS"));
    assert!(!file(".rs", "main.rs.bak"));
    assert!(!file(".rs", "rs"));
    assert!(!file(".rs", ".rs"), "a hidden name has no extension");
    assert!(!file(".rs", "main.rsx"));
}

#[test]
fn compound_extension_is_substring() {
    assert!(file(".tar.gz", "backup.tar.gz"));
    assert!(file(".tar.gz", "backup.tar.gz.part"));
}

#[test]
fn dot_name_is_extension_not_hidden_name() {
    assert!(!dir(".config", ".config"));
    assert!(file(".config", "web.config"));
    assert!(dir("config", ".config"));
    assert!(dir("./.config", ".config"));
    assert!(!dir("./.config", "home/.config"));
}

#[test]
fn component_aware_path_table() {
    for (query, yes, no) in [
        (
            "src/main",
            vec!["src/main.rs", "app/src/main.ts"],
            "source/main.rs",
        ),
        (
            "project/index",
            vec!["project/index.ts"],
            "myproject/index.ts",
        ),
        (
            "src/lib",
            vec!["src/lib.rs", "src/library.rs"],
            "src/old-lib.rs",
        ),
    ] {
        for rel in yes {
            assert!(file(query, rel), "{query} should match {rel}");
        }
        assert!(!file(query, no), "{query} should not match {no}");
    }
}

#[test]
fn intermediate_components_are_case_insensitive() {
    assert!(file("SRC/Main", "src/main.rs"));
}

#[test]
fn last_component_is_the_entry() {
    assert!(!file("src/main", "src/main.rs/inner.txt"));
    assert!(dir("src/main", "src/main"));
}

#[test]
fn leading_dot_slash_anchors_to_root() {
    assert!(file("./src/main", "src/main.rs"));
    assert!(!file("./src/main", "packages/app/src/main.rs"));
}

#[test]
fn trailing_slash_is_directory_name() {
    assert!(dir("src/", "src"));
    assert!(dir("src/", "app/src"));
    assert!(!dir("src/", "srcs"));
    assert!(!file("src/", "src"));
    assert!(!file("src/", "src/main.rs"));
}

#[test]
fn trailing_dot_component_is_directory_name() {
    for query in ["src/.", "src/./"] {
        assert!(dir(query, "src"), "{query}");
        assert!(!dir(query, "srcs"), "{query}");
        assert!(!file(query, "srcfoo.txt"), "{query}");
    }
}

#[test]
fn repeated_separators_and_dot_components_normalize() {
    assert!(file("src//main", "src/main.rs"));
    assert!(file("src/./main", "src/main.rs"));
    assert!(file(".//src/main", "src/main.rs"));
}

#[test]
fn parent_components_are_rejected() {
    assert_eq!(err("../src"), QueryError::ParentComponent);
    assert_eq!(err("src/../main"), QueryError::ParentComponent);
    assert_eq!(err("src/.."), QueryError::ParentComponent);
    assert_eq!(err(".."), QueryError::ParentComponent);
}

#[test]
fn unicode_case_insensitive() {
    assert!(file("ÄPFEL", "äpfel.txt"));
    assert!(file("ωmega", "ΩMEGA-notes.md"));
    assert!(file(".RS", "Main.rs"));
}

#[cfg(unix)]
#[test]
fn non_utf8_names_match_ascii_bytes() {
    use std::os::unix::ffi::OsStrExt;
    let name = Path::new(OsStr::from_bytes(b"caf\xe9-Index.txt"));
    assert!(q("index").matches(name, false));
    assert!(q(".txt").matches(name, false));
    assert!(
        !q("café").matches(name, false),
        "no lossy decoding of non-UTF-8 names"
    );
}

#[test]
fn path_query_names_its_hidden_components() {
    let anchored = q("./.github/workflows");
    assert!(anchored.names_hidden(Path::new(".github")));
    assert!(!anchored.names_hidden(Path::new("a/.github")));
    assert!(!anchored.names_hidden(Path::new(".git")));

    let floating = q(".github/workflows");
    assert!(floating.names_hidden(Path::new("a/.github")));

    let last = q("./.env");
    assert!(last.names_hidden(Path::new(".env")));
    assert!(
        last.names_hidden(Path::new(".envrc")),
        "last component keeps prefix semantics"
    );
}

#[test]
fn only_path_queries_grant_the_exemption() {
    assert!(!q(".env").names_hidden(Path::new(".env")));
    assert!(!q("env").names_hidden(Path::new(".env")));
}
