//! Orphan-note detection, shared by the `plc-orphans` binary and
//! `plc orphans`. An orphan is a `.md` note with neither an outbound
//! `[[link]]` nor any inbound link from another note.
//!
//! [`report`] returns the formatted output verbatim (no trailing newline), so
//! both front-ends print byte-identical results — matching the legacy zig
//! backend that `_orphans.sh` consumers expect.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::{Instant, UNIX_EPOCH};

use walkdir::WalkDir;

use crate::{ascii_lower, scan_content};

struct FileInfo {
    path: PathBuf,
    basename_lower: String,
    has_outbound: bool,
    size: u64,
    mtime_secs: i64,
}

/// Scan `root` for orphan notes and return the formatted report.
///
/// `root_display` is the prefix shown before each relative path (the root as
/// the user spelled it). When `verbose`, each orphan line also carries mtime
/// and size. Returns `Err` with a user-facing message if `root` is not a
/// directory.
pub fn report(
    root: &std::path::Path,
    root_display: &str,
    verbose: bool,
) -> Result<String, String> {
    let t_start = Instant::now();
    if !root.is_dir() {
        return Err(format!("Error: cannot open {root_display}"));
    }

    let mut targets: HashSet<String> = HashSet::new();
    let mut files: Vec<FileInfo> = Vec::new();

    for entry in WalkDir::new(root).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
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
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    (m.len(), secs)
                }
                Err(_) => (0, 0),
            }
        } else {
            (0, 0)
        };

        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
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
        if f.has_outbound || targets.contains(&f.basename_lower) {
            continue;
        }
        orphans.push(f);
    }
    orphans.sort_by(|a, b| a.path.cmp(&b.path));

    let root_clean = root_display.trim_end_matches('/');

    let mut lines: Vec<String> = Vec::new();
    lines.push(String::new());
    lines.push(format!(
        "  True orphans:  {} / {}  (no outbound and no inbound)",
        orphans.len(),
        files.len()
    ));
    lines.push(format!(
        "    no outbound : {}   distinct inbound targets : {}",
        no_out,
        targets.len()
    ));
    lines.push(String::new());
    for f in &orphans {
        if verbose {
            let (y, m, d) = ymd_from_secs(f.mtime_secs);
            lines.push(format!(
                "  {:04}-{:02}-{:02}   {:>6} B   {}/{}",
                y,
                m,
                d,
                f.size,
                root_clean,
                f.path.display()
            ));
        } else {
            lines.push(format!("  {}/{}", root_clean, f.path.display()));
        }
    }
    lines.push(String::new());
    lines.push("  ── runtime ───────────────────────".to_string());
    lines.push("  backend : rust".to_string());
    lines.push(format!("  elapsed : {} ms", t_start.elapsed().as_millis()));

    Ok(lines.join("\n"))
}

/// Convert unix seconds (UTC) to (year, month, day).
/// Howard Hinnant's date algorithm — see
/// http://howardhinnant.github.io/date_algorithms.html
fn ymd_from_secs(secs: i64) -> (i32, u8, u8) {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
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
