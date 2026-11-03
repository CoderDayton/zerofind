//! `--exclude` patterns: gitignore lines anchored at each search root.

use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::error::ZfError;

/// Rejects any pattern that is not a valid gitignore line, before the search starts.
pub fn check(patterns: &[String]) -> Result<(), ZfError> {
    build(Path::new("."), patterns).map(drop)
}

/// The patterns rooted at `root`; `None` when there are none.
pub fn build(root: &Path, patterns: &[String]) -> Result<Option<Gitignore>, ZfError> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GitignoreBuilder::new(root);
    for pattern in patterns {
        // gitignore reads these as a blank line or a comment: they would exclude nothing.
        if pattern.trim().is_empty() || pattern.starts_with('#') {
            return Err(ZfError::Exclude {
                pattern: pattern.clone(),
                reason: "it is blank or a comment and excludes nothing; write a leading # as \\#"
                    .into(),
            });
        }
        builder
            .add_line(None, pattern)
            .map_err(|err| invalid(pattern, err))?;
    }
    builder
        .build()
        .map(Some)
        .map_err(|err| invalid(&patterns.join(" "), err))
}

fn invalid(pattern: &str, err: ignore::Error) -> ZfError {
    let reason = match err {
        ignore::Error::Glob { err, .. } => err,
        other => other.to_string(),
    };
    ZfError::Exclude {
        pattern: pattern.to_owned(),
        reason,
    }
}
