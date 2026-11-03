//! Typed failures and stable exit statuses.

use std::fmt;
use std::io;

use crate::domain::query::QueryError;
use crate::domain::scope::ScopeError;

pub mod status {
    pub const MATCHED: u8 = 0;
    pub const NO_MATCH: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const PARTIAL: u8 = 3;
    pub const INTERRUPTED: u8 = 130;
}

/// Failures that stop a search as a whole. Recoverable traversal errors are counted instead.
#[derive(Debug)]
pub enum ZfError {
    Query(QueryError),
    Scope(ScopeError),
    Exclude { pattern: String, reason: String },
    Output(io::Error),
}

impl ZfError {
    pub fn status(&self) -> u8 {
        status::USAGE
    }
}

impl From<QueryError> for ZfError {
    fn from(e: QueryError) -> Self {
        Self::Query(e)
    }
}

impl From<ScopeError> for ZfError {
    fn from(e: ScopeError) -> Self {
        Self::Scope(e)
    }
}

impl fmt::Display for ZfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Query(QueryError::Empty) => write!(f, "empty query"),
            Self::Query(QueryError::EmptyExtension) => {
                write!(
                    f,
                    "invalid query '.': an extension query needs a name after the dot"
                )
            }
            Self::Query(QueryError::Absolute) => {
                write!(f, "invalid query: a path query cannot start with '/'")
            }
            Self::Query(QueryError::ParentComponent) => {
                write!(f, "invalid query: '..' is not a name")
            }
            Self::Query(QueryError::NoComponents) => {
                write!(f, "invalid query: the path names nothing")
            }
            Self::Scope(ScopeError::NoHome) => write!(f, "cannot find the home directory"),
            Self::Scope(ScopeError::Inaccessible { path, source }) => {
                write!(f, "cannot search '{}': {source}", path.display())
            }
            Self::Exclude { pattern, reason } => {
                write!(f, "invalid exclude pattern '{pattern}': {reason}")
            }
            Self::Output(e) => write!(f, "cannot write output: {e}"),
        }
    }
}
