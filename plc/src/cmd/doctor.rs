//! `plc doctor` — vault health check.
//!
//! A single place to diagnose a vault and propose (or, with `--fix`, apply) safe
//! repairs. Today it runs the ledger `.plc/config` checks; it is structured to
//! grow into a whole-vault checkup (orphan nodes, stale pointers, broken links,
//! …) by appending more sections here.

use clap::Args;

use crate::cmd::ledger;
use crate::config::Palace;

#[derive(Args)]
pub struct DoctorArgs {
    /// Apply the safe repairs each section proposes, instead of only reporting.
    #[arg(long = "fix")]
    fix: bool,
}

pub fn run(palace: &Palace, args: DoctorArgs) -> Result<String, String> {
    // Each section returns a ready-to-print report block. Add future checks
    // (note graph, pointers, …) to this list.
    let sections = [ledger::doctor(palace, args.fix)?];
    Ok(sections.join("\n"))
}
