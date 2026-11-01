//! Root resolution, against the library API.

use std::fs;
use std::path::{Path, PathBuf};

use zerofind::domain::scope::{RootKind, ScopeError, SearchRoot, resolve_explicit, resolve_home};

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
fn home_uses_existing_candidates_only() {
    let home = tempfile::tempdir().unwrap();
    let existing = mkdirs(home.path(), &["Documents", "Downloads"]);
    let candidates = vec![
        existing[0].clone(),
        home.path().join("Desktop"),
        existing[1].clone(),
    ];
    let roots = resolve_home(&candidates, Some(home.path())).unwrap();
    let paths: Vec<_> = roots.iter().map(|r| r.path.clone()).collect();
    assert_eq!(paths, existing);
    assert!(roots.iter().all(|r| r.path.is_absolute()));
}

#[test]
fn home_without_any_folder_is_an_error() {
    let home = tempfile::tempdir().unwrap();
    let candidates = vec![home.path().join("Documents")];
    let err = resolve_home(&candidates, Some(home.path())).unwrap_err();
    assert!(matches!(err, ScopeError::NoHomeFolders));
}

#[test]
fn home_deduplicates_identities_and_nesting() {
    let home = tempfile::tempdir().unwrap();
    let docs = mkdirs(home.path(), &["Documents", "Documents/Desktop"]);
    let alias = home.path().join("Docs-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&docs[0], &alias).unwrap();
    let candidates = vec![docs[0].clone(), docs[1].clone(), alias];
    let roots = resolve_home(&candidates, Some(home.path())).unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].path, docs[0]);
}

#[test]
fn home_never_falls_back_to_home_itself() {
    // xdg-user-dirs points disabled folders at $HOME.
    let home = tempfile::tempdir().unwrap();
    let candidates = vec![home.path().to_path_buf(), home.path().join(".")];
    let err = resolve_home(&candidates, Some(home.path())).unwrap_err();
    assert!(matches!(err, ScopeError::NoHomeFolders));
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
