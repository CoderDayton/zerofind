//! Root resolution, against the library API.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use zerofind::domain::scope::{
    RootKind, ScopeError, SearchRoot, dedupe, is_location, resolve_explicit, resolve_location,
};

fn mkdirs(base: &Path, names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|n| {
            let p = base.join(n);
            fs::create_dir_all(&p).unwrap();
            p
        })
        .collect()
}

#[test]
fn local_root_is_cwd_and_prints_without_dot_slash() {
    let root = SearchRoot::local().unwrap();
    assert_eq!(root.kind, RootKind::Dir);
    assert_eq!(
        root.display(Path::new("./src/main.rs")),
        Path::new("src/main.rs")
    );
}

#[test]
fn locations_are_absolute_home_parent_or_here() {
    for arg in ["..", "../x", "a/../b", "~", "~/x", ".", "./"] {
        assert!(is_location(OsStr::new(arg)), "{arg}");
    }
    #[cfg(unix)]
    assert!(is_location(OsStr::new("/etc/hosts")));
    for arg in ["src/main", "./src", "~x", ".env", "..x", "report"] {
        assert!(!is_location(OsStr::new(arg)), "{arg}");
    }
}

#[test]
fn location_splits_into_root_and_term() {
    let tmp = tempfile::tempdir().unwrap();
    let dl = mkdirs(tmp.path(), &["Downloads"]).remove(0);
    let loc = resolve_location(dl.join("report").as_os_str(), None).unwrap();
    assert_eq!(loc.root.path, dl);
    assert_eq!(loc.term.as_deref(), Some(OsStr::new("report")));

    let whole = resolve_location(dl.as_os_str(), None).unwrap();
    assert_eq!((whole.root.kind, whole.term), (RootKind::Dir, None));

    let err = resolve_location(tmp.path().join("no/such").as_os_str(), None).unwrap_err();
    assert!(matches!(err, ScopeError::Inaccessible { .. }));
}

#[test]
fn tilde_expands_to_the_given_home() {
    let home = tempfile::tempdir().unwrap();
    let docs = mkdirs(home.path(), &["Docs"]).remove(0);
    let loc = resolve_location(OsStr::new("~/Docs/x"), Some(home.path())).unwrap();
    assert_eq!(loc.root.path, docs);
    let err = resolve_location(OsStr::new("~/Docs"), None).unwrap_err();
    assert!(matches!(err, ScopeError::NoHome));
}

#[test]
fn dedupe_drops_repeats_and_nested_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = mkdirs(tmp.path(), &["a", "a/b", "c"]);
    let roots: Vec<SearchRoot> = [&dirs[1], &dirs[0], &dirs[2], &dirs[0]]
        .iter()
        .map(|d| resolve_explicit(d).unwrap())
        .collect();
    let kept: Vec<PathBuf> = dedupe(roots).into_iter().map(|r| r.path).collect();
    assert_eq!(kept, vec![dirs[0].clone(), dirs[2].clone()]);
}

#[test]
fn explicit_directory_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = resolve_explicit(tmp.path()).unwrap();
    assert_eq!(root.kind, RootKind::Dir);
    let entry = tmp.path().join("a.txt");
    assert_eq!(
        root.display(&entry),
        entry.as_path(),
        "explicit roots print as given"
    );
}

#[test]
fn explicit_file_root_is_evaluated_directly() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("hosts");
    fs::write(&f, "").unwrap();
    assert_eq!(resolve_explicit(&f).unwrap().kind, RootKind::File);
}

#[test]
fn missing_explicit_root_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let err = resolve_explicit(&tmp.path().join("nope")).unwrap_err();
    assert!(matches!(err, ScopeError::Inaccessible { .. }));
}

#[cfg(unix)]
#[test]
fn explicit_symlink_root_resolves_to_its_target_kind() {
    let tmp = tempfile::tempdir().unwrap();
    let target = mkdirs(tmp.path(), &["real"]).remove(0);
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let root = resolve_explicit(&link).unwrap();
    assert_eq!(root.kind, RootKind::Dir);
    assert_eq!(root.path, link, "printed paths keep the name the user gave");
}
