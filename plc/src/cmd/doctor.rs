//! `plc doctor` — vault health check.
//!
//! A single place to diagnose a vault and propose (or, with `--fix`, apply) safe
//! repairs. It runs *without* a resolved vault so it can diagnose a missing or
//! broken config. Today two sections: the `~/.plcrc` config, then the ledger
//! `.plc/config`. Structured to grow (orphan nodes, stale pointers, links, …).

use std::fs;

use clap::Args;

use crate::cmd::ledger;
use crate::config::{self, Palace};

#[derive(Args)]
pub struct DoctorArgs {
    /// Apply the safe repairs each section proposes, instead of only reporting.
    #[arg(long = "fix")]
    fix: bool,
}

pub fn run(args: DoctorArgs) -> Result<String, String> {
    let mut out = vec![config_section(args.fix)];
    // Ledger checks need a valid vault; skip (with a note) when it doesn't resolve.
    match Palace::resolve() {
        Ok(palace) => out.push(ledger::doctor(&palace, args.fix)?),
        Err(e) => out.push(format!("\n  Ledger — skipped ({e})")),
    }
    Ok(out.join("\n"))
}

/// Check that the vault path is configured and resolves — `$PALACE_DIR` or
/// `~/.plcrc`. `--fix` persists an env-only path into `~/.plcrc`.
fn config_section(fix: bool) -> String {
    let mut lines = vec![String::new(), "  Doctor — config (~/.plcrc)".to_string(), String::new()];

    let env_dir = std::env::var("PALACE_DIR").ok().filter(|s| !s.trim().is_empty());
    let rc_dir = config::read_plcrc_palace_dir();
    let rc_path = config::plcrc_path();
    let rc_shown = rc_path.as_ref().map_or_else(|| "~/.plcrc".to_string(), |p| p.display().to_string());

    match (&env_dir, &rc_dir) {
        (None, None) => {
            lines.push("  ! PALACE_DIR is not set (environment or ~/.plcrc)".to_string());
            lines.push(format!("      set it: echo 'export PALACE_DIR=\"/path/to/vault\"' >> {rc_shown}"));
        }
        (Some(env), rc) if rc.as_deref() != Some(env.trim()) => {
            // Set in the environment but not (matching) in ~/.plcrc.
            lines.push("  ! PALACE_DIR comes from the environment, not persisted to ~/.plcrc".to_string());
            if fix {
                match persist_plcrc(env.trim()) {
                    Ok(p) => lines.push(format!("      fixed: wrote export PALACE_DIR to {p}")),
                    Err(e) => lines.push(format!("      could not write {rc_shown}: {e}")),
                }
            } else {
                lines.push("      run `plc doctor --fix` to write it".to_string());
            }
        }
        _ => lines.push(format!("  · PALACE_DIR from ~/.plcrc → {}", rc_dir.as_deref().unwrap_or(""))),
    }

    // Whatever the source, does the resolved vault actually validate?
    match Palace::resolve() {
        Ok(p) => lines.push(format!("  · vault OK → {}", p.root().display())),
        Err(e) => lines.push(format!("  ! {e}")),
    }
    lines.join("\n")
}

/// Write `export PALACE_DIR="<dir>"` into `~/.plcrc`, preserving any other lines.
/// Returns the file path written.
fn persist_plcrc(dir: &str) -> Result<String, String> {
    let path = config::plcrc_path().ok_or_else(|| "$HOME is not set".to_string())?;
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let mut kept: Vec<String> = existing
        .lines()
        .filter(|l| !l.trim().strip_prefix("export ").unwrap_or(l.trim()).starts_with("PALACE_DIR"))
        .map(str::to_string)
        .collect();
    kept.push(format!("export PALACE_DIR=\"{dir}\""));
    fs::write(&path, kept.join("\n") + "\n").map_err(|e| format!("{e}"))?;
    Ok(path.display().to_string())
}
