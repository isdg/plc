//! plc-orphans — list .md files in a palace tree that have neither
//! outbound `[[link]]` nor any inbound link from another note.
//!
//! The scan + output live in `plc_core::orphans` (shared with `plc
//! orphans`); this binary is just the legacy CLI front-end.

use std::path::Path;
use std::process::ExitCode;

fn print_help() {
    print!(
        "Usage: plc-orphans [-r ROOT] [-v]\n\
         \n\
         Find true orphan .md notes — no outbound [[link]] in\n\
         content AND no inbound link from any other note.\n\
         \n\
         \x20\x20-r, --root DIR  Search root (default: palace/notes)\n\
         \x20\x20-v, --verbose   Show mtime + size\n\
         \x20\x20-h, --help      This help\n"
    );
}

fn main() -> ExitCode {
    let mut root = String::from("palace/notes");
    let mut verbose = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-r" | "--root" => match args.next() {
                Some(v) => root = v,
                None => return ExitCode::from(1),
            },
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("Unknown option: {other}");
                return ExitCode::from(1);
            }
        }
    }

    match plc_core::orphans::report(Path::new(&root), &root, verbose) {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
