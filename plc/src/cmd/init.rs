//! `plc init` — scaffold the canonical palace note tree.
//!
//! Materializes the general directory layout under the target vault (default
//! `$PALACE_DIR`, or an explicit `DIR` argument) so a fresh repo is ready to
//! fill and use. Idempotent: existing directories are left untouched, never
//! deleted. Unlike the note commands, this does *not* require an already-valid
//! vault — it's how you create one.
//!
//! Only general/structural dirs are created: not the personal content taxonomy
//! (art, books, bio/chem, dnd, …) and not the per-year `daily/<YYYY>` subdirs
//! (`plc daily` makes those on demand).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use clap::Args;

/// Canonical leaf directories of a palace vault. Parents are created implicitly
/// by `create_dir_all`, so only the deepest path of each branch is listed.
const DIRS: &[&str] = &[
    "notes/archive",
    "notes/management/daily",
    "notes/management/do",
    "notes/management/weekly",
    "notes/me/writing/isg",
    "notes/me/writing/murmur",
    "notes/projects",
    "notes/sensible",
    "templates",
];

#[derive(Args)]
pub struct InitArgs {
    /// Target vault directory (default: $PALACE_DIR).
    #[arg(value_name = "DIR")]
    dir: Option<String>,
}

pub fn run(args: InitArgs) -> Result<String, String> {
    let root: PathBuf = match args.dir {
        Some(d) => PathBuf::from(d),
        None => env::var_os("PALACE_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| "init: no target — pass a DIR or set PALACE_DIR".to_string())?,
    };

    let (created, existed) = scaffold(&root)?;
    Ok(format!(
        "init: {} — {created} created, {existed} already present ({} dirs)",
        root.display(),
        DIRS.len()
    ))
}

/// Create every canonical directory under `root`. Returns (created, existed).
fn scaffold(root: &Path) -> Result<(usize, usize), String> {
    let mut created = 0;
    let mut existed = 0;
    for rel in DIRS {
        let p = root.join(rel);
        if p.is_dir() {
            existed += 1;
        } else {
            fs::create_dir_all(&p).map_err(|e| format!("init: {}: {e}", p.display()))?;
            created += 1;
        }
    }
    Ok((created, existed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_creates_then_is_idempotent() {
        let root = env::temp_dir().join(format!("plc-init-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        // First run creates everything.
        let (created, existed) = scaffold(&root).unwrap();
        assert_eq!(created, DIRS.len());
        assert_eq!(existed, 0);

        // Representative paths exist.
        assert!(root.join("notes/me/writing/isg").is_dir());
        assert!(root.join("notes/management/daily").is_dir());
        assert!(root.join("templates").is_dir());

        // Second run is a no-op.
        let (created2, existed2) = scaffold(&root).unwrap();
        assert_eq!(created2, 0);
        assert_eq!(existed2, DIRS.len());

        fs::remove_dir_all(&root).ok();
    }
}
