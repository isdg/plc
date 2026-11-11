//! `plc doctor` — vault health check.
//!
//! A single place to diagnose a vault and propose (or, with `--fix`, apply) safe
//! repairs. It runs *without* a resolved vault so it can diagnose a missing or
//! broken config. Today two sections: the `PALACE_DIR` config, then the ledger
//! `.plc/config`. Structured to grow (orphan nodes, stale pointers, links, …).

use clap::Args;

use crate::cmd::ledger;
use crate::config::Palace;

#[derive(Args)]
pub struct DoctorArgs {
    /// Apply the safe repairs each section proposes, instead of only reporting.
    #[arg(long = "fix")]
    fix: bool,
}

pub fn run(args: DoctorArgs) -> Result<String, String> {
    let mut out = vec![config_section()];
    // Ledger checks need a valid vault; skip (with a note) when it doesn't resolve.
    match Palace::resolve() {
        Ok(palace) => out.push(ledger::doctor(&palace, args.fix)?),
        Err(e) => out.push(format!("\n  Ledger — skipped ({e})")),
    }
    Ok(out.join("\n"))
}

/// Check that `$PALACE_DIR` is set and resolves to a valid vault.
fn config_section() -> String {
    let mut lines = vec![
        String::new(),
        "  Doctor — config (PALACE_DIR)".to_string(),
        String::new(),
    ];
    match Palace::resolve() {
        Ok(p) => lines.push(format!("  · vault OK → {}", p.root().display())),
        Err(e) => lines.push(format!("  ! {e}")),
    }
    lines.join("\n")
}
