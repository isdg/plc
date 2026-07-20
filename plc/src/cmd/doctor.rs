//! `plc doctor` — vault health check.
//!
//! A single place to diagnose a vault and propose (or, with `--fix`, apply) safe
//! repairs. It runs *without* a resolved vault so it can diagnose a missing or
//! broken config. Today two sections: the `~/.plcrc` config, then the ledger
//! `.plc/config`. Structured to grow (orphan nodes, stale pointers, links, …).

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

    // The persistent path lives in ~/.plcrc; an environment `PALACE_DIR` is a
    // legitimate temporal override (tests, a second vault) and is not a problem
    // as long as a persistent one exists. Only nag when nothing is persisted.
    match (&rc_dir, &env_dir) {
        (Some(rc), env) => {
            lines.push(format!("  · PALACE_DIR (~/.plcrc) → {rc}"));
            if let Some(e) = env.as_deref().map(str::trim).filter(|e| *e != rc) {
                lines.push(format!("  · environment overrides it this run → {e}"));
            }
        }
        (None, Some(env)) => {
            lines.push("  ! PALACE_DIR is set in the environment but not persisted to ~/.plcrc".to_string());
            if fix {
                match config::write_plcrc_palace_dir(env.trim()) {
                    Ok(p) => lines.push(format!("      fixed: wrote export PALACE_DIR to {}", p.display())),
                    Err(e) => lines.push(format!("      could not write {rc_shown}: {e}")),
                }
            } else {
                lines.push("      run `plc doctor --fix` to persist it".to_string());
            }
        }
        (None, None) => {
            lines.push("  ! PALACE_DIR is not set (environment or ~/.plcrc)".to_string());
            lines.push("      set it: plc config --set /path/to/vault".to_string());
        }
    }

    // Whatever the source, does the resolved vault actually validate?
    match Palace::resolve() {
        Ok(p) => lines.push(format!("  · vault OK → {}", p.root().display())),
        Err(e) => lines.push(format!("  ! {e}")),
    }
    lines.join("\n")
}
