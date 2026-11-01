//! Query interpretation and matching, against the library API.

use std::ffi::OsStr;
use std::path::Path;

use zerofind::domain::query::{CompiledQuery, Named, Query, QueryError, QueryMode};

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
fn leading_dot_selects_extension_mode() {
    assert_eq!(q(".rs").mode(), QueryMode::Extension);
    assert_eq!(q(".config").mode(), QueryMode::Extension);
    assert_eq!(q(".tar.gz").mode(), QueryMode::Extension);
}

#[test]
fn wildcard_selects_glob_mode() {
    assert_eq!(q("*.rs").mode(), QueryMode::Glob);
    assert_eq!(q("report-202?").mode(), QueryMode::Glob);
    assert_eq!(q("src/*.rs").mode(), QueryMode::Path);
}

#[test]
fn everything_else_is_name_mode() {
    assert_eq!(q("index").mode(), QueryMode::Name);
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
        ("port", ["report.txt", "port.c"], "pot.c"),
    ] {
        for name in yes {
            assert!(file(query, name), "{query} should match {name}");
        }
        assert!(!file(query, no), "{query} should not match {no}");
    }
}

#[test]
fn dotted_name_starts_at_a_word_boundary() {
    for name in [
        "main.rs",
        "my-main.rs",
        "my_main.rs",
        "x.main.rs",
        "main.rs.bak",
        "MAIN.RS",
    ] {
        assert!(file("main.rs", name), "main.rs should match {name}");
    }
    for name in ["domain.rs", "remain.rs", "émain.rs"] {
        assert!(!file("main.rs", name), "main.rs should not match {name}");
    }
    assert!(!file("config.json", "tsconfig.json"));
    assert!(file("v1.2", "v1.2.3.tar.gz"));
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
    assert!(!file(".rs", "main.rsx"));
}

#[test]
fn compound_extension_is_exact() {
    assert!(file(".tar.gz", "backup.tar.gz"));
    assert!(file(".TAR.GZ", "x.v1.tar.gz"));
    assert!(!file(".tar.gz", "backup.tar.gz.part"));
    assert!(!file(".tar.gz", "backup.tgz"));
}

#[test]
fn dot_name_matches_extension_and_the_hidden_name() {
    assert!(dir(".config", ".config"));
    assert!(file(".env", ".env"));
    assert!(file(".env", "prod.env"));
    assert!(!file(".env", ".envrc"));
    assert!(file(".eslintrc.json", ".eslintrc.json"));
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

fn names(query: &str, rel: &str) -> Named {
    q(query).names(Path::new(rel))
}

#[test]
fn path_query_reaches_the_components_it_names() {
    assert_eq!(names("./.github/workflows", ".github"), Named::Reach);
    assert_eq!(names("./.github/workflows", "a/.github"), Named::No);
    assert_eq!(names("./.github/workflows", ".git"), Named::No);
    assert_eq!(names(".github/workflows", "a/.github"), Named::Reach);
    assert_eq!(names("node_modules/react", "node_modules"), Named::Reach);
    assert_eq!(
        names("./.env", ".envrc"),
        Named::Reach,
        "last component keeps prefix semantics"
    );
}

#[test]
fn a_term_spelling_the_whole_name_names_it() {
    assert_eq!(names(".env", "app/.env"), Named::Leaf);
    assert_eq!(names("node_modules", "node_modules"), Named::Leaf);
    assert_eq!(names("BUILD", "docs/build"), Named::Leaf);
    assert_eq!(names(".*rc", ".bashrc"), Named::Leaf);
    assert_eq!(names("env", ".env"), Named::No, "a fragment names nothing");
    assert_eq!(
        names("*", ".env"),
        Named::No,
        "a bare wildcard names nothing"
    );
    assert_eq!(names(".rs", "main.rs"), Named::No);
}

#[test]
fn glob_matches_the_whole_name() {
    assert!(file("*.rs", "main.rs"));
    assert!(file("*.RS", "src/lib.rs"));
    assert!(!file("*.rs", "main.rs.bak"));
    assert!(file("report-202?.pdf", "Report-2024.pdf"));
    assert!(!file("report-202?.pdf", "report-20245.pdf"));
    assert!(file("a*b*c", "axxbyyc"));
    assert!(!file("a*b*c", "axxbyy"));
    assert!(file("[draft]*", "[draft] notes.md"), "brackets are literal");
    assert!(!file("*.rs", "src.rs/inner.txt"), "ancestors do not match");
}

#[test]
fn glob_components_in_path_queries() {
    assert!(file("src/*.rs", "app/src/main.rs"));
    assert!(!file("src/*.rs", "src/main.rs.bak"));
    assert!(file("*/src/main", "app/src/main.rs"));
    assert!(!file("./*/main", "a/b/main.rs"));
}

fn all(terms: &[&str]) -> Query {
    Query::new(terms.iter().map(|t| q(t)).collect())
}

#[test]
fn every_term_must_match() {
    let query = all(&["annual", "report"]);
    assert!(query.matches(Path::new("docs/2024 Annual Report.pdf"), false));
    assert!(!query.matches(Path::new("docs/annual-budget.pdf"), false));
    assert!(all(&["report", ".pdf"]).matches(Path::new("report.pdf"), false));
    assert!(!all(&["report", ".pdf"]).matches(Path::new("report.txt"), false));
}

#[test]
fn no_terms_match_everything() {
    assert!(all(&[]).matches(Path::new("anything.bin"), false));
    assert!(all(&[]).matches(Path::new("a/b"), true));
}

#[test]
fn query_names_what_any_term_names() {
    assert_eq!(all(&["x", ".env"]).names(Path::new(".env")), Named::Leaf);
    assert_eq!(
        all(&["./.github/w"]).names(Path::new(".github")),
        Named::Reach
    );
    assert_eq!(all(&[]).names(Path::new(".env")), Named::No);
}

#[test]
fn decomposed_unicode_matches_composed_query() {
    let nfd = "cafe\u{301}.txt";
    assert!(file("café", nfd));
    assert!(file("CAFÉ", nfd));
    assert!(file("cafe\u{301}", "café.txt"));
}
