//! `plc fin` — plain-text finance tracking, kept beside your daily notes.
//!
//! Transactions live in a per-day ledger file next to the daily note but marked
//! by a `+ledger` filename postfix and tagged `[[ledger]]`:
//!
//! ```text
//! notes/management/daily/2026/07/2026-07-19+ledger.md
//! ```
//!
//! `plc fin add` formats a transaction line and appends it to today's ledger in
//! one shot (seeding the header first if the file is new). A transaction line is:
//!
//! ```text
//! $ <±amount> [CUR]  @[[account]]  (#[[category]] | > @[[account2]])  [memo…]
//! ```
//!
//! `-` is an expense (outflow), `+` income (inflow), and `> @[[dest]]` a transfer;
//! accounts/categories stay `[[wikilinks]]` so the link/orphans engine still sees
//! them. See `plc_core::finance` for the full grammar. Bare `plc fin` just seeds
//! and prints today's ledger path so you can open it by hand.

use chrono::Local;
use clap::{Args, Subcommand};
use plc_core::finance::{self, Kind, Transaction};

use crate::config::Palace;
use crate::note;

#[derive(Args)]
pub struct FinArgs {
    #[command(subcommand)]
    cmd: Option<FinCmd>,
}

#[derive(Subcommand)]
enum FinCmd {
    /// Append a transaction to today's ledger.
    Add(AddArgs),
    /// Summarize transactions across all ledgers (net, by category, by account).
    Report,
}

#[derive(Args)]
pub struct AddArgs {
    /// Amount in the major unit, positive (e.g. 4.50). The direction comes from
    /// `--income`/`--to`, not a sign here.
    #[arg(value_name = "AMOUNT")]
    amount: String,
    /// Free-text payee/memo (all trailing words).
    #[arg(value_name = "MEMO")]
    memo: Vec<String>,
    /// Account the money moves through (required).
    #[arg(short = 'a', long = "account", value_name = "ACCOUNT")]
    account: String,
    /// Category for an expense or income.
    #[arg(short = 'c', long = "category", value_name = "CATEGORY")]
    category: Option<String>,
    /// Transfer the amount to this account instead of categorizing it.
    #[arg(long = "to", value_name = "ACCOUNT", conflicts_with_all = ["category", "income"])]
    to: Option<String>,
    /// Record as income (inflow). Default is an expense (outflow).
    #[arg(short = 'i', long = "income")]
    income: bool,
    /// Currency ISO code (default: $PLC_CURRENCY, else EUR).
    #[arg(long = "cur", value_name = "CUR")]
    currency: Option<String>,
    /// Project/event tag, nested with `/` (e.g. `japan-trip/work`). Repeatable.
    #[arg(short = 'p', long = "project", value_name = "PROJECT")]
    project: Vec<String>,
}

pub fn run(palace: &Palace, args: FinArgs) -> Result<String, String> {
    match args.cmd {
        None => seed_today(palace),
        Some(FinCmd::Add(add_args)) => add(palace, add_args),
        Some(FinCmd::Report) => report(palace),
    }
}

/// `plc fin report`: aggregate every `+ledger` file under the daily tree.
fn report(palace: &Palace) -> Result<String, String> {
    let root = palace.root().join("notes/management/daily");
    finance::report(&root, &finance::default_currency())
}

/// Today's ledger location: `(subdir, filename)` under the daily tree.
fn ledger_location() -> (String, String) {
    let now = Local::now();
    (
        now.format("notes/management/daily/%Y/%m").to_string(),
        now.format("%Y-%m-%d+ledger.md").to_string(),
    )
}

/// Bare `plc fin`: seed today's ledger (if new) and print its path.
fn seed_today(palace: &Palace) -> Result<String, String> {
    let (subdir, filename) = ledger_location();
    note::ensure_note(palace.root(), &subdir, &filename, "ledger", None, note::SIGNATURE)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("fin: {e}"))
}

/// `plc fin add`: build the transaction line and append it to today's ledger.
fn add(palace: &Palace, args: AddArgs) -> Result<String, String> {
    let amount = finance::amount_to_minor(&args.amount)
        .ok_or_else(|| format!("fin: invalid amount: {}", args.amount))?;
    let account = clean_link("account", &args.account)?;
    let currency = args
        .currency
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(finance::default_currency);

    let (kind, other) = match (args.to, args.category, args.income) {
        (Some(dest), _, _) => (Kind::Transfer, Some(clean_link("account", &dest)?)),
        (None, cat, income) => {
            let kind = if income { Kind::Income } else { Kind::Expense };
            let other = cat.map(|c| clean_link("category", &c)).transpose()?;
            (kind, other)
        }
    };

    let projects = args
        .project
        .iter()
        .map(|p| clean_link("project", p).map(|p| finance::normalize_tag(&p)))
        .collect::<Result<Vec<_>, _>>()?;

    let txn =
        Transaction { amount, currency, kind, account, other, projects, memo: args.memo.join(" ") };
    let entry = finance::format_entry(&txn);

    let (subdir, filename) = ledger_location();
    note::append_line(palace.root(), &subdir, &filename, "ledger", &entry)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("fin: {e}"))
}

/// Validate a value destined for a `[[wikilink]]`: non-blank and free of the
/// brackets/newline that would break the link (mirrors the global `--tag` check).
fn clean_link(label: &str, raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("fin: {label} must not be blank"));
    }
    if trimmed.contains(['[', ']', '\n']) {
        return Err(format!("fin: {label} must not contain brackets or newlines: {trimmed}"));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_args() -> AddArgs {
        AddArgs {
            amount: "4.50".into(),
            memo: vec!["Blue".into(), "Bottle".into()],
            account: "cash".into(),
            category: Some("coffee".into()),
            to: None,
            income: false,
            currency: Some("EUR".into()),
            project: vec![],
        }
    }

    /// Reproduce `add`'s line-building without touching the filesystem.
    fn line_of(args: AddArgs) -> Result<String, String> {
        let amount = finance::amount_to_minor(&args.amount)
            .ok_or_else(|| format!("fin: invalid amount: {}", args.amount))?;
        let account = clean_link("account", &args.account)?;
        let currency = args
            .currency
            .map(|c| c.trim().to_uppercase())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(finance::default_currency);
        let (kind, other) = match (args.to, args.category, args.income) {
            (Some(dest), _, _) => (Kind::Transfer, Some(clean_link("account", &dest)?)),
            (None, cat, income) => {
                let kind = if income { Kind::Income } else { Kind::Expense };
                (kind, cat.map(|c| clean_link("category", &c)).transpose()?)
            }
        };
        let projects = args
            .project
            .iter()
            .map(|p| clean_link("project", p).map(|p| finance::normalize_tag(&p)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(finance::format_entry(&Transaction {
            amount,
            currency,
            kind,
            account,
            other,
            projects,
            memo: args.memo.join(" "),
        }))
    }

    #[test]
    fn builds_expense_line() {
        assert_eq!(line_of(add_args()).unwrap(), "$ -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle");
    }

    #[test]
    fn builds_income_line() {
        let mut a = add_args();
        a.income = true;
        a.category = Some("salary".into());
        a.memo = vec![];
        a.amount = "2400".into();
        assert_eq!(line_of(a).unwrap(), "$ +2400.00 EUR  @[[cash]] #[[salary]]");
    }

    #[test]
    fn builds_transfer_line() {
        let mut a = add_args();
        a.category = None;
        a.to = Some("checking".into());
        a.memo = vec!["ATM".into()];
        a.amount = "200".into();
        assert_eq!(line_of(a).unwrap(), "$ 200.00 EUR  @[[cash]] > @[[checking]]  ATM");
    }

    #[test]
    fn rejects_bracket_in_account() {
        let mut a = add_args();
        a.account = "ca[sh".into();
        assert!(line_of(a).is_err());
    }

    #[test]
    fn builds_line_with_project() {
        // `-p` flows through, lowercased with the `/` hierarchy preserved.
        let mut a = add_args();
        a.memo = vec![];
        a.project = vec!["Japan-Trip/Work".into()];
        assert_eq!(
            line_of(a).unwrap(),
            "$ -4.50 EUR  @[[cash]] #[[coffee]] ~[[japan-trip/work]]"
        );
    }

    #[test]
    fn long_entry_wraps_when_added() {
        // Many tags push past 66 cols → block form, every line within budget.
        let mut a = add_args();
        a.memo = vec!["latte".into()];
        a.project = vec!["japan-trip/leisure".into(), "work".into(), "reimbursable".into()];
        let entry = line_of(a).unwrap();
        assert!(entry.contains('\n'), "should wrap: {entry}");
        assert!(entry.lines().all(|l| l.chars().count() <= 66), "over 66: {entry}");
    }
}
