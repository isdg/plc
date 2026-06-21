//! `plc orphans` — list .md notes with no outbound `[[link]]` and no inbound
//! link from another note. Wraps the shared `palace_core::orphans` engine.
//!
//! Defaults the search root to `<PALACE_DIR>/notes`; an explicit `-r DIR`
//! scans any directory and skips vault resolution entirely.

use std::path::{Path, PathBuf};

use clap::Args;

use crate::config::Palace;

#[derive(Args)]
pub struct OrphansArgs {
    /// Search root (default: <PALACE_DIR>/notes).
    #[arg(short = 'r', long = "root", value_name = "DIR")]
    root: Option<String>,
    /// Show mtime + size next to each path.
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
}

pub fn run(args: OrphansArgs) -> Result<String, String> {
    let root: PathBuf = match args.root {
        Some(r) => PathBuf::from(r),
        None => Palace::resolve()?.root().join("notes"),
    };
    let display = root.to_string_lossy();
    palace_core::orphans::report(Path::new(&root), &display, args.verbose)
}
