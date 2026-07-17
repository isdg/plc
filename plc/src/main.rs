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
    /// Append a postfix to every created note's filename, before the extension
    /// (e.g. `--postfix review` → `2026-07-14T20.28+review.md`). Applies to
    /// whichever subcommand runs.
    #[arg(short = 'x', long = "postfix", global = true, value_name = "TEXT")]
    postfix: Option<String>,
    /// Seed an extra `[[TAG]]` wikilink line into every newly-created note body
    /// (e.g. `--tag review`). Only affects notes seeded with a header — not
    /// `shot -i`/`--no-header` bodies.
    #[arg(short = 't', long = "tag", global = true, value_name = "TAG")]
    tag: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create/resolve today's (or a given date's) daily note.
    Daily(cmd::daily::DailyArgs),
    /// Create/resolve this ISO week's note.
    Weekly,
    /// Create a timestamped snapshot note in the daily dir (or a chosen path).
    Shot(cmd::shot::ShotArgs),
    /// Create/resolve TOP.md at the vault root (the palace landing page).
    Top,
    /// Manage do-notes (week-based) with a "last" pointer.
    Do(cmd::do_notes::DoArgs),
    /// Manage free-form murmur notes.
    Murmur(cmd::murmur::MurmurArgs),
    /// Create/resolve enumerated isg notes (isg0, isg1, …).
    Isg(cmd::isg::IsgArgs),
    /// List orphan notes (no outbound and no inbound links).
    Orphans(cmd::orphans::OrphansArgs),
    /// Scaffold the canonical vault directory tree.
    Init(cmd::init::InitArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // A postfix ends up in a filename, and a tag in a `[[…]]` wikilink, so
    // validate both before they reach the note layer.
    let stamped = validate_postfix(cli.postfix).and_then(|postfix| {
        let tag = validate_tag(cli.tag)?;
        Ok((postfix, tag))
    });
    match stamped {
        Ok((postfix, tag)) => {
            note::set_postfix(postfix);
            note::set_tag_postfix(tag);
        }
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    }

    // Note commands need a validated vault; `orphans` resolves its own root
    // (and can run without a vault when given `-r`).
    let result = match cli.cmd {
        Cmd::Daily(args) => with_palace(|p| cmd::daily::run(p, args)),
        Cmd::Weekly => with_palace(cmd::weekly::run),
        Cmd::Shot(args) => cmd::shot::run(args),
        Cmd::Top => with_palace(cmd::top::run),
        Cmd::Do(args) => with_palace(|p| cmd::do_notes::run(p, args)),
        Cmd::Murmur(args) => with_palace(|p| cmd::murmur::run(p, args)),
        Cmd::Isg(args) => with_palace(|p| cmd::isg::run(p, args)),
        Cmd::Orphans(args) => cmd::orphans::run(args),
        Cmd::Init(args) => cmd::init::run(args),
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

/// Validate the global `--postfix`: trim it, and reject a blank value or one
/// containing a path separator (it must stay a single filename component).
/// Returns the cleaned postfix (or `None` when the flag was absent).
fn validate_postfix(postfix: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = postfix else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("plc: --postfix must not be blank".to_string());
    }
    if trimmed.contains('/') || trimmed.contains(std::path::MAIN_SEPARATOR) {
        return Err(format!("plc: --postfix must not contain a path separator: {trimmed}"));
    }
    Ok(Some(trimmed.to_string()))
}

/// Validate the global `--tag`: trim it, and reject a blank value or one whose
/// characters would break the `[[…]]` wikilink it is seeded into (brackets or
/// a newline). Returns the cleaned tag (or `None` when the flag was absent).
fn validate_tag(tag: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = tag else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("plc: --tag must not be blank".to_string());
    }
    if trimmed.contains(['[', ']', '\n']) {
        return Err(format!("plc: --tag must not contain brackets or newlines: {trimmed}"));
    }
    Ok(Some(trimmed.to_string()))
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
