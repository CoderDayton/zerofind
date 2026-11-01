//! Argument parsing and help. No search logic here.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "zf",
    version,
    about = "Find files and directories by name, extension, or path.",
    after_help = "Queries (every word must match):
  index            names containing \"index\"
  main.rs          names containing main.rs at a word start (not domain.rs)
  .rs  .tar.gz     exact extension; .env also finds the file named .env
  *.rs  report-2?  wildcards over the whole name
  src/main         path components; ./src/main starts at the search root
  ~/Downloads/cv   search ~/Downloads for cv (also /abs/path, ../dir, .)

Hidden entries, dependencies and build output are skipped unless the query names them."
)]
pub struct Cli {
    /// Words to match, and places to search
    #[arg(value_name = "QUERY")]
    pub terms: Vec<OsString>,

    /// Search the home directory
    #[arg(long)]
    pub home: bool,

    /// Search a directory (or evaluate one file); repeatable
    #[arg(short = 'p', long, value_name = "DIRECTORY")]
    pub path: Vec<PathBuf>,

    /// Also search hidden entries, dependencies, build output and ignored files
    #[arg(long)]
    pub all: bool,

    /// Return files only
    #[arg(long, conflicts_with = "dirs")]
    pub files: bool,

    /// Return directories only
    #[arg(long)]
    pub dirs: bool,

    /// Use NUL-delimited output
    #[arg(short = '0', long)]
    pub print0: bool,
}

/// Short flags `zf` knows; any other `-word` is a query (`zf -draft`).
const SHORT_FLAGS: &[u8] = b"p0hV";

/// `-0`, `-p`, `-0p`: a cluster of known short flags.
fn is_flag_cluster(b: &[u8]) -> bool {
    b.len() > 1 && b[0] == b'-' && b[1..].iter().all(|c| SHORT_FLAGS.contains(c))
}

/// Moves query words that start with `-` behind `--`, so they need no escaping. Long options
/// are left alone: a mistyped `--depth` is still a usage error. A `-p` value must be a
/// separate argument (`-p dir`): `-pending` is a query.
pub fn normalize(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut out = Vec::new();
    let mut dashed = Vec::new();
    let mut args = args.into_iter();
    out.extend(args.next());
    let mut takes_value = false;
    let mut separator = false;
    for arg in args {
        let b = arg.as_encoded_bytes();
        let query_word = !separator
            && !takes_value
            && b.len() > 1
            && b[0] == b'-'
            && b[1] != b'-'
            && !is_flag_cluster(b);
        let value = takes_value;
        takes_value =
            !separator && !value && (b == b"--path" || (is_flag_cluster(b) && b.ends_with(b"p")));
        separator |= !value && b == b"--";
        if query_word {
            dashed.push(arg);
        } else {
            out.push(arg);
        }
    }
    if !dashed.is_empty() {
        if !separator {
            out.push("--".into());
        }
        out.extend(dashed);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(args: &[&str]) -> Vec<String> {
        normalize(args.iter().map(OsString::from))
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn dash_words_move_behind_the_separator() {
        assert_eq!(norm(&["zf", "-draft", "x"]), ["zf", "x", "--", "-draft"]);
        assert_eq!(
            norm(&["zf", "-2024", "--", "y"]),
            ["zf", "--", "y", "-2024"]
        );
        assert_eq!(norm(&["zf", "-pending"]), ["zf", "--", "-pending"]);
    }

    #[test]
    fn flags_and_their_values_stay() {
        for args in [
            &["zf", "-0", "x"][..],
            &["zf", "-p", "-odd-dir", "x"][..],
            &["zf", "-0p", "-odd-dir", "x"][..],
            &["zf", "--depth", "3"][..],
            &["zf", "-hV"][..],
            &["zf", "--", "-x"][..],
        ] {
            assert_eq!(norm(args), args, "{args:?}");
        }
    }
}
