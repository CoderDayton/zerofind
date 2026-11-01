//! Search semantics, independent of CLI parsing, traversal libraries and stdout.

pub mod policy;
pub mod query;
pub mod scope;

use policy::{ResultKind, VisibilityPolicy};
use query::CompiledQuery;
use scope::SearchRoot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Raw bytes, one record per line: for pipes and files.
    Lines,
    /// Lines with control characters and invalid UTF-8 escaped: for a terminal.
    Terminal,
    Nul,
}

/// Everything a search needs, validated before traversal and never rewritten by it.
#[derive(Debug, Clone)]
pub struct SearchPlan {
    pub roots: Vec<SearchRoot>,
    pub query: CompiledQuery,
    pub visibility: VisibilityPolicy,
    pub result_kind: ResultKind,
    pub output: OutputFormat,
}
