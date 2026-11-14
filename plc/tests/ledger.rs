//! End to end: `plc ledger` report, reg and balance filter with `-g`.

mod common;

use common::{fail, ok, Home};

#[test]
fn reports_filter_with_grep() {
    let home = Home::new("ledger-grep");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    for (amount, memo, account) in
        [("4.50", "coffee", "cash"), ("12", "lunch", "bank")]
    {
        let args = ["ledger", "add", amount, memo, "-a", account, "-n"];
        ok(home.plc(&args).args(["-c", "food"]));
    }
    let reg = ok(&mut home.plc(&["ledger", "reg", "-g", "coffee"]));
    assert!(reg.contains("Register — 1 transaction(s)"), "{reg}");
    let both = ["ledger", "reg", "-g", "coffee", "-g", "lunch"];
    assert!(ok(&mut home.plc(&both)).contains("2 transaction(s)"));
    for cmd in ["report", "reg", "bal"] {
        let err = fail(&mut home.plc(&["ledger", cmd, "coffee"]));
        assert!(err.contains("invalid day: coffee"), "{err}");
    }
}

#[test]
fn reports_take_the_stat_date() {
    let home = Home::new("ledger-dates");
    home.vault("palace");
    home.write(".plc/.plcrc", "vault = ~/palace\n");
    for (day, memo) in [("2026-11-05", "coffee"), ("2026-10-02", "tea")] {
        let args = ["ledger", "add", "3", memo, "-a", "cash", "-n"];
        ok(home.plc(&args).args(["-c", "food", "-d", day]));
    }
    let reg = |date: &[&str]| {
        let out = ok(home.plc(&["ledger", "reg"]).args(date));
        out.lines().nth(1).unwrap_or_default().to_string()
    };
    assert!(reg(&[]).contains("2 transaction(s)"));
    assert!(reg(&["05", "11", "26"]).contains("1 transaction(s)"));
    assert!(reg(&["-", "10", "26"]).contains("1 transaction(s)"));
    assert!(reg(&["-", "-", "26"]).contains("2 transaction(s)"));
    assert!(reg(&["06", "11", "26"]).contains("0 transaction(s)"));
    let both = ["ledger", "reg", "-", "11", "26", "--since", "2026-01-01"];
    assert!(fail(&mut home.plc(&both)).contains("cannot be used with"));
}
