//! End to end: the built `plc` binary resolves the vault from `$PALACE_DIR`
//! alone, never from `~/.plcrc`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn home(tag: &str) -> PathBuf {
    let name = format!("plc-config-{tag}-{}", std::process::id());
    let dir = std::env::temp_dir().join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn plc(home: &Path, palace_dir: Option<&str>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_plc"));
    cmd.args(args).env("HOME", home).env_remove("PALACE_DIR");
    if let Some(dir) = palace_dir {
        cmd.env("PALACE_DIR", dir);
    }
    cmd.output().unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const UNSET: &str = "config: PALACE_DIR is not set; \
                     add `export PALACE_DIR=/path/to/vault` to your shell rc";

#[test]
fn prints_palace_dir_with_tilde_expanded() {
    let home = home("env");
    let out = plc(&home, Some("~/vault"), &["config"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let printed = String::from_utf8(out.stdout).unwrap();
    assert_eq!(printed.trim_end(), home.join("vault").display().to_string());
    fs::remove_dir_all(&home).ok();
}

#[test]
fn unset_or_empty_is_an_error() {
    let home = home("unset");
    for palace_dir in [None, Some(""), Some("  ")] {
        let out = plc(&home, palace_dir, &["config"]);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert_eq!(stderr(&out).trim_end(), UNSET);
    }
    fs::remove_dir_all(&home).ok();
}

#[test]
fn plcrc_is_ignored_but_named() {
    let home = home("plcrc");
    let vault = home.join("vault");
    fs::create_dir_all(vault.join("notes")).unwrap();
    let rc = format!("PALACE_DIR = {}\n", vault.display());
    fs::write(home.join(".plcrc"), rc).unwrap();

    let out = plc(&home, None, &["config"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let hinted = format!("{UNSET} (~/.plcrc is no longer read)");
    assert_eq!(stderr(&out).trim_end(), hinted);
    fs::remove_dir_all(&home).ok();
}

#[test]
fn set_and_rc_flags_are_gone() {
    let home = home("set");
    for flag in ["--set", "--rc"] {
        let out = plc(&home, None, &["config", flag, "/elsewhere"]);
        assert!(!out.status.success());
    }
    assert!(!home.join(".plcrc").exists());
    fs::remove_dir_all(&home).ok();
}
