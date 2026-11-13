//! plc's home, `$PLC_HOME` or else `~/.plc`, and its settings file `.plcrc`:
//! the user's defaults, such as the vault. plc never writes `.plcrc`, so it
//! can be a link into a dotfiles repo.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use chrono::Weekday;

/// The settings file's name inside the plc home.
pub const RC: &str = ".plcrc";

/// Where `plc add` puts notes when `.plcrc` sets no `notes`: the triage
/// category, under the vault.
pub const NOTES: &str = "notes/triage";

/// The parsed `.plcrc`. A missing file loads as [`Rc::default`].
#[derive(Debug, Default, PartialEq)]
pub struct Rc {
    /// The default vault, with a leading `~` expanded.
    pub vault: Option<PathBuf>,
    /// The directory `plc add` writes to, with a leading `~` expanded; a
    /// relative path is under the vault (see [`Rc::notes_dir`]).
    pub notes: Option<PathBuf>,
    /// The day weeks start on, as written (see [`Rc::week_start`]).
    pub week_start: Option<String>,
    /// Keys plc does not recognise, as written; commands ignore them.
    pub unknown: Vec<String>,
}

/// plc's machine-local directory, outside every vault: `$PLC_HOME` (when
/// non-empty), else `~/.plc`.
pub fn home() -> Result<PathBuf, String> {
    match env::var_os("PLC_HOME").filter(|h| !h.is_empty()) {
        Some(h) => std::path::absolute(h).map_err(|e| format!("PLC_HOME: {e}")),
        None => Ok(user_home()?.join(".plc")),
    }
}

/// The settings file, `<plc home>/.plcrc`. Without `$PLC_HOME`, a legacy
/// `~/.plcrc` is first moved there.
pub fn path() -> Result<PathBuf, String> {
    let home = home()?;
    if !plc_home_set() {
        adopt(&legacy_path()?, &home)?;
    }
    Ok(home.join(RC))
}

/// Where plc kept the vault path before it had a home: `~/.plcrc`.
pub fn legacy_path() -> Result<PathBuf, String> {
    Ok(user_home()?.join(RC))
}

/// A legacy `~/.plcrc` left in place because `.plcrc` already existed.
/// Never one under `$PLC_HOME`, which does not adopt it.
pub fn unmoved_legacy() -> Result<Option<PathBuf>, String> {
    if plc_home_set() {
        return Ok(None);
    }
    path()?;
    let legacy = legacy_path()?;
    Ok(legacy.exists().then_some(legacy))
}

fn plc_home_set() -> bool {
    env::var_os("PLC_HOME").is_some_and(|h| !h.is_empty())
}

/// Move `legacy` into `home` as `.plcrc`, unless one is already there, even
/// as a dangling link.
fn adopt(legacy: &Path, home: &Path) -> Result<(), String> {
    let path = home.join(RC);
    let free = matches!(
        fs::symlink_metadata(&path),
        Err(e) if e.kind() == ErrorKind::NotFound
    );
    if !free || !legacy.exists() {
        return Ok(());
    }
    fs::create_dir_all(home)
        .and_then(|()| fs::rename(legacy, &path))
        .map_err(|e| format!("move {} to {}: {e}", tilde(legacy), tilde(&path)))
}

fn user_home() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "$HOME is not set".to_string())
}

impl Rc {
    /// Read the settings at `path` (see [`path`]), or defaults when absent.
    pub fn read(path: &Path) -> Result<Rc, String> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Rc::parse(&text)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Rc::default()),
            Err(e) => Err(format!("read {}: {e}", tilde(path))),
        }
    }

    /// Parse `key = value` lines; `#` comments and blank lines are skipped,
    /// and a later key wins over an earlier one.
    pub fn parse(text: &str) -> Rc {
        let mut rc = Rc::default();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=').unwrap_or((line, ""));
            match (key.trim(), value.trim()) {
                ("vault", "") => rc.vault = None,
                ("vault", v) => rc.vault = Some(expand_tilde(v)),
                ("notes", "") => rc.notes = None,
                ("notes", v) => rc.notes = Some(expand_tilde(v)),
                ("week_start", "") => rc.week_start = None,
                ("week_start", v) => rc.week_start = Some(v.to_string()),
                (k, _) => rc.unknown.push(k.to_string()),
            }
        }
        rc
    }

    /// The directory `plc add` writes to: `notes` as is when absolute, else
    /// under the vault `root`, which is only resolved then. Unset, it is
    /// `<root>/notes/triage`.
    pub fn notes_dir<F>(&self, root: F) -> Result<PathBuf, String>
    where
        F: FnOnce() -> Result<PathBuf, String>,
    {
        match &self.notes {
            Some(dir) if dir.is_absolute() => Ok(dir.clone()),
            dir => {
                let rel = dir.as_deref().unwrap_or(Path::new(NOTES));
                Ok(root()?.join(rel))
            }
        }
    }

    /// The day weeks start on: `week_start`, a weekday name in full or
    /// cut to three letters, in any case. Unset, it is Monday, as in ISO.
    pub fn week_start(&self) -> Result<Weekday, String> {
        let Some(day) = &self.week_start else {
            return Ok(Weekday::Mon);
        };
        day.parse().map_err(|_| {
            format!("week_start: `{day}` is not a weekday (monday … sunday)")
        })
    }

    /// Whether it still sets `PALACE_DIR`, the key a legacy `~/.plcrc` used
    /// for what `.plcrc` calls `vault`.
    pub fn has_legacy_key(&self) -> bool {
        self.unknown
            .iter()
            .map(|k| k.trim_start_matches("export ").trim())
            .any(|k| k == "PALACE_DIR")
    }
}

/// The `week_start` of the settings file (see [`path`]).
pub fn week_start() -> Result<Weekday, String> {
    Rc::read(&path()?)?.week_start()
}

/// Expand a leading `~` / `~/` to `$HOME`; otherwise the path as-is.
pub fn expand_tilde(s: &str) -> PathBuf {
    expand_tilde_home(s, env::var_os("HOME").as_deref())
}

/// [`expand_tilde`] with an injected home, so it is testable without touching
/// the process environment.
fn expand_tilde_home(s: &str, home: Option<&OsStr>) -> PathBuf {
    let s = s.trim();
    match s.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => match home {
            Some(h) => Path::new(h).join(rest.trim_start_matches('/')),
            None => PathBuf::from(s),
        },
        _ => PathBuf::from(s),
    }
}

/// `path` for display, with a leading `$HOME` shortened to `~`.
pub fn tilde(path: &Path) -> String {
    tilde_home(path, env::var_os("HOME").as_deref())
}

/// [`tilde`] with an injected home.
fn tilde_home(path: &Path, home: Option<&OsStr>) -> String {
    let home = home.filter(|h| !h.is_empty());
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_cases() {
        let home = Some(OsStr::new("/home/x"));
        let expand = |s| expand_tilde_home(s, home);
        assert_eq!(expand("~/vault"), PathBuf::from("/home/x/vault"));
        assert_eq!(expand("~"), PathBuf::from("/home/x"));
        assert_eq!(expand("/abs/path"), PathBuf::from("/abs/path"));
        // `~user` (no slash) is left untouched.
        assert_eq!(expand("~bob/x"), PathBuf::from("~bob/x"));
        assert_eq!(expand_tilde_home("~/v", None), PathBuf::from("~/v"));
    }

    #[test]
    fn tilde_cases() {
        let home = Some(OsStr::new("/home/x"));
        let short = |p: &str| tilde_home(Path::new(p), home);
        assert_eq!(short("/home/x/.plc/.plcrc"), "~/.plc/.plcrc");
        assert_eq!(short("/home/x"), "~");
        assert_eq!(short("/home/xy/.plcrc"), "/home/xy/.plcrc");
        assert_eq!(tilde_home(Path::new("/a"), None), "/a");
    }

    #[test]
    fn parse_keys() {
        let text = "# defaults\n\nvault = /v\nfoo = 1\nbare\nvault=/w\n";
        let rc = Rc::parse(text);
        assert_eq!(rc.vault, Some(PathBuf::from("/w")));
        assert_eq!(rc.unknown, ["foo", "bare"]);
        assert_eq!(Rc::parse("vault = /v\nvault =\n").vault, None);
    }

    #[test]
    fn parse_notes() {
        let rc = Rc::parse("notes = inbox\n");
        assert_eq!(rc.notes, Some(PathBuf::from("inbox")));
        assert!(rc.unknown.is_empty());
        assert_eq!(Rc::parse("notes = /n\nnotes =\n").notes, None);
    }

    #[test]
    fn week_start_cases() {
        let start = |text: &str| Rc::parse(text).week_start();
        assert_eq!(start(""), Ok(Weekday::Mon));
        assert_eq!(start("week_start = sunday"), Ok(Weekday::Sun));
        assert_eq!(start("week_start = Sat"), Ok(Weekday::Sat));
        assert_eq!(start("week_start = sun\nweek_start ="), Ok(Weekday::Mon));
        for bad in ["su", "sundays", "7"] {
            let err = start(&format!("week_start = {bad}")).unwrap_err();
            assert!(err.contains(&format!("`{bad}`")), "{err}");
        }
        assert!(Rc::parse("week_start = x").unknown.is_empty());
    }

    #[test]
    fn notes_dir_cases() {
        let root = || Ok(PathBuf::from("/v"));
        let dir = |text| Rc::parse(text).notes_dir(root).unwrap();
        assert_eq!(dir(""), PathBuf::from("/v/notes/triage"));
        assert_eq!(dir("notes = inbox/q"), PathBuf::from("/v/inbox/q"));
        assert_eq!(dir("notes = /abs"), PathBuf::from("/abs"));
    }

    #[test]
    fn absolute_notes_need_no_vault() {
        let no_vault = || Err("no vault".to_string());
        let rc = Rc::parse("notes = /abs");
        assert_eq!(rc.notes_dir(no_vault), Ok(PathBuf::from("/abs")));
        let rc = Rc::parse("notes = rel");
        assert_eq!(rc.notes_dir(no_vault), Err("no vault".to_string()));
    }

    #[test]
    fn legacy_key() {
        for text in ["PALACE_DIR = /v", "export PALACE_DIR=\"/v\""] {
            let rc = Rc::parse(text);
            assert!(rc.has_legacy_key(), "{text}");
            assert_eq!(rc.vault, None);
        }
        assert!(!Rc::parse("vault = /v\nPALACE = /w").has_legacy_key());
    }

    fn tmp(tag: &str) -> PathBuf {
        let name = format!("plc-rc-{tag}-{}", std::process::id());
        let dir = env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn adopt_moves_a_legacy_rc() {
        let dir = tmp("adopt");
        let (legacy, home) = (dir.join(RC), dir.join(".plc"));
        adopt(&legacy, &home).unwrap();
        assert!(!home.exists(), "nothing to adopt, nothing created");

        fs::write(&legacy, "PALACE_DIR = /v\n").unwrap();
        adopt(&legacy, &home).unwrap();
        assert!(!legacy.exists());
        let moved = fs::read_to_string(home.join(RC)).unwrap();
        assert_eq!(moved, "PALACE_DIR = /v\n");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn adopt_never_replaces_an_rc() {
        let dir = tmp("keep");
        let (legacy, home) = (dir.join(RC), dir.join(".plc"));
        fs::create_dir_all(&home).unwrap();
        fs::write(&legacy, "old").unwrap();

        std::os::unix::fs::symlink(dir.join("gone"), home.join(RC)).unwrap();
        adopt(&legacy, &home).unwrap();
        assert!(legacy.exists());
        assert!(fs::symlink_metadata(home.join(RC)).unwrap().is_symlink());

        fs::remove_file(home.join(RC)).unwrap();
        fs::write(home.join(RC), "new").unwrap();
        adopt(&legacy, &home).unwrap();
        assert!(legacy.exists());
        assert_eq!(fs::read_to_string(home.join(RC)).unwrap(), "new");
        fs::remove_dir_all(&dir).ok();
    }
}
