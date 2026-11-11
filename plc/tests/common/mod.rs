//! A throwaway `$HOME` to run the built `plc` in, isolated from the real
//! one: `PALACE_DIR` and `PLC_HOME` are cleared unless a test sets them.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const NO_VAULT: &str = "no vault: set PALACE_DIR, \
                            or add `vault = /path/to/vault` to";

pub struct Home(pub PathBuf);

impl Home {
    pub fn new(tag: &str) -> Home {
        let name = format!("plc-test-{tag}-{}", std::process::id());
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Home(dir)
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    /// Write `text` to `rel`, creating its parent directories.
    pub fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    /// A minimal valid vault at `rel`.
    pub fn vault(&self, rel: &str) -> PathBuf {
        let root = self.path(rel);
        fs::create_dir_all(root.join("notes")).unwrap();
        root
    }

    pub fn plc(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_plc"));
        cmd.args(args)
            .env("HOME", &self.0)
            .env_remove("PALACE_DIR")
            .env_remove("PLC_HOME");
        cmd
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).trim_end().to_string()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim_end().to_string()
}

/// Run to success and return stdout.
pub fn ok(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    stdout(&out)
}

/// Run to failure and return stderr.
pub fn fail(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(out.stdout.is_empty());
    stderr(&out)
}

pub fn display(path: &Path) -> String {
    path.display().to_string()
}
