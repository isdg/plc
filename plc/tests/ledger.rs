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
        assert!(err.contains("unexpected argument 'coffee'"), "{err}");
    }
}
