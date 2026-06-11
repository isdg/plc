//! palace-orphans — list .md files in a palace tree that have neither
//! outbound `[[link]]` nor any inbound link from another note.
//!
//! Output shape matches the legacy zig binary so `_orphans.sh` can
//! switch backends without consumer-visible changes.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Instant, UNIX_EPOCH};

use palace_core::{ascii_lower, scan_content};
use walkdir::WalkDir;

struct FileInfo {
    path: PathBuf,
    basename_lower: String,
    has_outbound: bool,
    size: u64,
    mtime_secs: i64,
}

fn print_help() {
    print!(
        "Usage: palace-orphans [-r ROOT] [-v]\n\
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
    let t_start = Instant::now();

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

    let root_path = Path::new(&root);
    if !root_path.is_dir() {
        eprintln!("Error: cannot open {root}");
        return ExitCode::from(1);
    }

    let mut targets: HashSet<String> = HashSet::new();
    let mut files: Vec<FileInfo> = Vec::new();

    for entry in WalkDir::new(root_path).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str())
        else {
            continue;
        };
        if !name.ends_with(".md") {
            continue;
        }

        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };

        let has_outbound = scan_content(&content, &mut targets);
        let base = &name[..name.len() - 3];
        let base_lower = ascii_lower(base);

        let (size, mtime_secs) = if verbose {
            match entry.metadata() {
                Ok(m) => {
                    let secs = m
                        .modified()
                        .ok()
                        .and_then(|t| {
                            t.duration_since(UNIX_EPOCH).ok()
                        })
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    (m.len(), secs)
                }
                Err(_) => (0, 0),
            }
        } else {
            (0, 0)
        };

        let rel = path
            .strip_prefix(root_path)
            .unwrap_or(path)
            .to_path_buf();

        files.push(FileInfo {
            path: rel,
            basename_lower: base_lower,
            has_outbound,
            size,
            mtime_secs,
        });
    }

    let mut orphans: Vec<&FileInfo> = Vec::new();
    let mut no_out = 0usize;
    for f in &files {
        if !f.has_outbound {
            no_out += 1;
        }
        if f.has_outbound {
            continue;
        }
        if targets.contains(&f.basename_lower) {
            continue;
        }
        orphans.push(f);
    }
    orphans.sort_by(|a, b| a.path.cmp(&b.path));

    let root_clean = root.trim_end_matches('/');

    println!();
    println!(
        "  True orphans:  {} / {}  (no outbound and no inbound)",
        orphans.len(),
        files.len()
    );
    println!(
        "    no outbound : {}   distinct inbound targets : {}",
        no_out,
        targets.len()
    );
    println!();

    for f in &orphans {
        if verbose {
            let (y, m, d) = ymd_from_secs(f.mtime_secs);
            println!(
                "  {:04}-{:02}-{:02}   {:>6} B   {}/{}",
                y,
                m,
                d,
                f.size,
                root_clean,
                f.path.display()
            );
        } else {
            println!("  {}/{}", root_clean, f.path.display());
        }
    }

    let elapsed = t_start.elapsed().as_millis();
    println!();
    println!("  ── runtime ───────────────────────");
    println!("  backend : rust");
    println!("  elapsed : {elapsed} ms");

    ExitCode::SUCCESS
}

/// Convert unix seconds (UTC) to (year, month, day).
/// Howard Hinnant's date algorithm — see
/// http://howardhinnant.github.io/date_algorithms.html
fn ymd_from_secs(secs: i64) -> (i32, u8, u8) {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe =
        (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    if m <= 2 {
        y += 1;
    }
    (y as i32, m, d)
}
