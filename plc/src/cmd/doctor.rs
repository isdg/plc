//! `plc doctor` — vault health check.
//!
//! A single place to diagnose a vault and propose (or, with `--fix`, apply) safe
//! repairs. It runs *without* a resolved vault so it can diagnose a missing or
//! broken config. Today two sections: the `.plcrc` config, then the ledger
//! `.plc/config`. Structured to grow (orphan nodes, stale pointers, links, …).

use std::path::Path;

use clap::Args;
use plc_core::rc::{self, Rc};

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

/// Check `.plcrc` (see [`rc`]), then that the vault and its state dir
/// resolve.
fn config_section() -> String {
    let mut lines = vec![String::new()];
    match rc::path() {
        Ok(path) => {
            let shown = rc::tilde(&path);
            lines.push(format!("  Doctor — config ({shown})"));
            lines.push(String::new());
            lines.extend(rc_findings(&path, &shown));
        }
        Err(e) => {
            lines.extend(["  Doctor — config".to_string(), String::new()]);
            lines.push(format!("  ! {e}"));
        }
    }
    match Palace::resolve() {
        Ok(p) => {
            lines.push(format!("  · vault OK → {}", p.root().display()));
            lines.push(match p.state_dir() {
                Ok(dir) => format!("  · machine state → {}", rc::tilde(&dir)),
                Err(e) => format!("  ! {e}"),
            });
        }
        Err(e) => lines.push(format!("  ! {e}")),
    }
    lines.join("\n")
}

/// Keys `.plcrc` sets that plc ignores or cannot read, and a legacy
/// `~/.plcrc` that could not move into its place.
fn rc_findings(path: &Path, shown: &str) -> Vec<String> {
    let mut lines = Vec::new();
    match Rc::read(path) {
        Ok(rc) => {
            for key in &rc.unknown {
                let what = format!("unknown key `{key}` (ignored)");
                lines.push(format!("  ! {shown}: {what}"));
            }
            if rc.has_legacy_key() {
                let hint = "rename its PALACE_DIR key to `vault`";
                lines.push(format!("      {hint}"));
            }
            if let Err(e) = rc.week_start() {
                lines.push(format!("  ! {shown}: {e}"));
            }
        }
        Err(e) => lines.push(format!("  ! {e}")),
    }
    match rc::unmoved_legacy() {
        Ok(Some(legacy)) => {
            let legacy = rc::tilde(&legacy);
            lines.push(format!("  ! {legacy} not moved: {shown} exists"));
            lines.push(format!("      merge it into {shown}, then delete it"));
        }
        Ok(None) => {}
        Err(e) => lines.push(format!("  ! {e}")),
    }
    lines
}
