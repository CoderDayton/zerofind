//! Argument parsing and help. No search logic here.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "zf",
    version,
    about = "Find files and directories by name, extension, or path.",
    after_help = "Queries:\n  index        names containing \"index\"\n  .rs          exact extension\n  src/main     path components; ./src/main anchors to the root"
)]
pub struct Cli {
    /// Name fragment, .ext, or path/fragment
    pub query: OsString,

    /// Search standard user folders
    #[arg(long, conflicts_with = "path")]
    pub home: bool,

    /// Search an explicit directory
    #[arg(short = 'p', long, value_name = "DIRECTORY")]
    pub path: Option<PathBuf>,

    /// Include normally excluded entries
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
