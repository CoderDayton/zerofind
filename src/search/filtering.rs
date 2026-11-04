//! Walked paths relative to their search root.

use std::path::Path;

/// `path` below `root`. Walked paths are always `root` joined with more, so on Unix the prefix
/// is cut as bytes instead of compared component by component.
#[cfg(unix)]
pub fn relative<'a>(path: &'a Path, root: &Path) -> &'a Path {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let (p, r) = (path.as_os_str().as_bytes(), root.as_os_str().as_bytes());
    match p.strip_prefix(r) {
        Some(rest) => Path::new(OsStr::from_bytes(rest.strip_prefix(b"/").unwrap_or(rest))),
        None => path.strip_prefix(root).unwrap_or(path),
    }
}

#[cfg(not(unix))]
pub fn relative<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}
