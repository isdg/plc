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
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let palace = match Palace::resolve() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };

    let result = match cli.cmd {
        Cmd::Daily(args) => cmd::daily::run(&palace, args),
        Cmd::Weekly => cmd::weekly::run(&palace),
        Cmd::Shot => cmd::shot::run(&palace),
    };

    match result {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
