//! plc — palace notes manager.
//!
//! Each subcommand creates or resolves a file in the palace vault and prints
//! its absolute path to stdout. The binary never opens an editor and is never
//! interactive: the zsh wrappers call `plc`, then open the printed path with
//! `$EDITOR` (and pipe list output through fzf where a picker is wanted).

mod cmd;
mod config;
mod note;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

use config::Palace;

#[derive(Parser)]
#[command(
    name = "plc",
    version,
    about = "palace notes manager — creates files, prints their paths"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create/resolve today's (or a given date's) daily note.
    Daily(cmd::daily::DailyArgs),
    /// Create/resolve this ISO week's note.
    Weekly,
    /// Create a timestamped daily snapshot note.
    Shot,
    /// Manage do-notes (week-based) with a "last" pointer.
    Do(cmd::do_notes::DoArgs),
    /// Manage free-form murmur notes.
    Murmur(cmd::murmur::MurmurArgs),
    /// List orphan notes (no outbound and no inbound links).
    Orphans(cmd::orphans::OrphansArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Note commands need a validated vault; `orphans` resolves its own root
    // (and can run without a vault when given `-r`).
    let result = match cli.cmd {
        Cmd::Daily(args) => with_palace(|p| cmd::daily::run(p, args)),
        Cmd::Weekly => with_palace(cmd::weekly::run),
        Cmd::Shot => with_palace(cmd::shot::run),
        Cmd::Do(args) => with_palace(|p| cmd::do_notes::run(p, args)),
        Cmd::Murmur(args) => with_palace(|p| cmd::murmur::run(p, args)),
        Cmd::Orphans(args) => cmd::orphans::run(args),
    };

    match result {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

/// Resolve the vault, then run `f` against it. Centralizes the `PALACE_DIR`
/// validation shared by every note-creating command.
fn with_palace<F>(f: F) -> Result<String, String>
where
    F: FnOnce(&Palace) -> Result<String, String>,
{
    let palace = Palace::resolve()?;
    f(&palace)
}
