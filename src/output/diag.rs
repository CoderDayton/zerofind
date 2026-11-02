//! Diagnostics on stderr. They never fail a search: once stderr is closed (`2>&1 | head`),
//! they are dropped.

use std::fmt::Display;
use std::io::{self, Write};

pub fn error(msg: impl Display) {
    let _ = writeln!(io::stderr().lock(), "zf: {msg}");
}

pub fn warning(msg: impl Display) {
    let _ = writeln!(io::stderr().lock(), "zf: warning: {msg}");
}

/// One summary line per walk when `ZF_TRACE=1`.
pub fn trace(msg: impl Display) {
    let _ = writeln!(io::stderr().lock(), "zf: trace: {msg}");
}
