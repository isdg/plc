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

use chrono::{DateTime, Datelike, FixedOffset, Local, LocalResult, NaiveDate, TimeZone};
use clap::{Args, Subcommand};
use plc_core::finance::{self, Filter, Kind, State, Transaction};

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
    Report(ReportArgs),
    /// List matching transactions chronologically with a running total.
    Reg(ReportArgs),
}

#[derive(Args)]
pub struct ReportArgs {
    /// Keep only transactions whose account/category/tag/memo contains a PATTERN
    /// (case-insensitive). Multiple patterns match if any one does.
    #[arg(value_name = "PATTERN")]
    patterns: Vec<String>,
    /// Only cleared (`*`) transactions.
    #[arg(long = "cleared", conflicts_with = "pending")]
    cleared: bool,
    /// Only pending (`!`) transactions.
    #[arg(long = "pending")]
    pending: bool,
    /// Only on/after this date (YYYY-MM-DD).
    #[arg(long = "since", value_name = "YYYY-MM-DD")]
    since: Option<String>,
    /// Only on/before this date (YYYY-MM-DD).
    #[arg(long = "until", value_name = "YYYY-MM-DD")]
    until: Option<String>,
    /// Restrict to one month (YYYY-MM); sets since/until to its bounds.
    #[arg(long = "month", value_name = "YYYY-MM", conflicts_with_all = ["since", "until"])]
    month: Option<String>,
    /// Report only: cap the account/category/tag trees at N levels.
    #[arg(long = "depth", value_name = "N")]
    depth: Option<usize>,
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
    /// Explicit date YYYY-MM-DD (default: today, the ledger's day).
    #[arg(short = 'd', long = "date", value_name = "YYYY-MM-DD")]
    date: Option<String>,
    /// Mark the transaction cleared (`*`).
    #[arg(long = "cleared", conflicts_with = "pending")]
    cleared: bool,
    /// Mark the transaction pending (`!`).
    #[arg(long = "pending")]
    pending: bool,
}

pub fn run(palace: &Palace, args: FinArgs) -> Result<String, String> {
    match args.cmd {
        None => seed_today(palace),
        Some(FinCmd::Add(add_args)) => add(palace, add_args),
        Some(FinCmd::Report(report_args)) => report(palace, report_args),
        Some(FinCmd::Reg(report_args)) => reg(palace, report_args),
    }
}

/// `plc fin report`: aggregate the matching `+ledger` transactions.
fn report(palace: &Palace, args: ReportArgs) -> Result<String, String> {
    let filter = build_filter(&args)?;
    let root = palace.root().join("notes/management/daily");
    finance::report(&root, &finance::default_currency(), &filter)
}

/// `plc fin reg`: chronological register of the matching transactions.
fn reg(palace: &Palace, args: ReportArgs) -> Result<String, String> {
    let filter = build_filter(&args)?;
    let root = palace.root().join("notes/management/daily");
    finance::register(&root, &finance::default_currency(), &filter)
}

/// Build a [`Filter`] from the shared report/register flags.
fn build_filter(args: &ReportArgs) -> Result<Filter, String> {
    let state = if args.cleared {
        Some(State::Cleared)
    } else if args.pending {
        Some(State::Pending)
    } else {
        None
    };
    let (since, until) = date_range(args)?;
    Ok(Filter {
        state,
        patterns: args.patterns.iter().map(|p| p.to_lowercase()).collect(),
        since,
        until,
        depth: args.depth,
    })
}

/// Resolve the `--since`/`--until`/`--month` flags into an inclusive date range.
fn date_range(args: &ReportArgs) -> Result<(Option<NaiveDate>, Option<NaiveDate>), String> {
    if let Some(m) = &args.month {
        let first = NaiveDate::parse_from_str(&format!("{}-01", m.trim()), "%Y-%m-%d")
            .map_err(|_| format!("fin: invalid month (want YYYY-MM): {m}"))?;
        let (y, mo) = (first.year(), first.month());
        let (ny, nm) = if mo == 12 { (y + 1, 1) } else { (y, mo + 1) };
        let last = NaiveDate::from_ymd_opt(ny, nm, 1)
            .and_then(|d| d.pred_opt())
            .ok_or_else(|| format!("fin: invalid month: {m}"))?;
        return Ok((Some(first), Some(last)));
    }
    let parse = |o: &Option<String>| match o {
        Some(s) => NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
            .map(Some)
            .map_err(|_| format!("fin: invalid date (want YYYY-MM-DD): {s}")),
        None => Ok(None),
    };
    Ok((parse(&args.since)?, parse(&args.until)?))
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

/// `plc fin add`: build the transaction and append it to today's ledger.
fn add(palace: &Palace, args: AddArgs) -> Result<String, String> {
    let txn = build_txn(args, Local::now().fixed_offset())?;
    let entry = finance::format_entry(&txn);
    let (subdir, filename) = ledger_location();
    note::append_line(palace.root(), &subdir, &filename, "ledger", &entry)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("fin: {e}"))
}

/// Build a [`Transaction`] from `add` args. `now` is the default timestamp used
/// when `--date` is absent (injected so tests are deterministic).
fn build_txn(args: AddArgs, now: DateTime<FixedOffset>) -> Result<Transaction, String> {
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
        .map(|p| clean_link("project", p).map(|p| finance::normalize_name(&p)))
        .collect::<Result<Vec<_>, _>>()?;

    // Stamp the full instant by default (like a note); `--date` overrides.
    let date = Some(match args.date.as_deref() {
        Some(s) => parse_when(s)?,
        None => now,
    });
    let state = if args.cleared {
        State::Cleared
    } else if args.pending {
        State::Pending
    } else {
        State::Uncleared
    };

    Ok(Transaction {
        amount,
        currency,
        kind,
        account,
        other,
        date,
        state,
        projects,
        memo: args.memo.join(" "),
    })
}

/// Parse a `--date` value: a full `YYYY-MM-DD HH:MM:SS ±ZZZZ` timestamp, or a
/// bare `YYYY-MM-DD` taken as that day at local midnight.
fn parse_when(s: &str) -> Result<DateTime<FixedOffset>, String> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_str(s, finance::TIMESTAMP_FMT) {
        return Ok(dt);
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        if let Some(naive) = d.and_hms_opt(0, 0, 0) {
            if let LocalResult::Single(dt) = Local.from_local_datetime(&naive) {
                return Ok(dt.fixed_offset());
            }
        }
    }
    Err(format!("fin: invalid date (want YYYY-MM-DD or full timestamp): {s}"))
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
            date: None,
            cleared: false,
            pending: false,
        }
    }

    /// A fixed "now" so `build_txn`'s default stamp is deterministic in tests.
    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_str("2026-07-19 11:28:22 +0200", finance::TIMESTAMP_FMT).unwrap()
    }

    #[test]
    fn maps_expense_args() {
        let t = build_txn(add_args(), now()).unwrap();
        assert_eq!(t.kind, Kind::Expense);
        assert_eq!((t.amount, t.account.as_str(), t.other.as_deref()), (450, "cash", Some("coffee")));
        assert_eq!(t.memo, "Blue Bottle");
        assert_eq!(t.date, Some(now())); // stamped by default
        assert_eq!(t.state, State::Uncleared);
    }

    #[test]
    fn maps_income_args() {
        let mut a = add_args();
        a.income = true;
        a.category = Some("salary".into());
        a.amount = "2400".into();
        let t = build_txn(a, now()).unwrap();
        assert_eq!(t.kind, Kind::Income);
        assert_eq!(t.amount, 240000);
        assert_eq!(t.other.as_deref(), Some("salary"));
    }

    #[test]
    fn maps_transfer_args() {
        let mut a = add_args();
        a.category = None;
        a.to = Some("checking".into());
        a.amount = "200".into();
        let t = build_txn(a, now()).unwrap();
        assert_eq!(t.kind, Kind::Transfer);
        assert_eq!((t.account.as_str(), t.other.as_deref()), ("cash", Some("checking")));
    }

    #[test]
    fn projects_normalized_slash_preserved() {
        let mut a = add_args();
        a.project = vec!["Japan-Trip/Work".into()];
        assert_eq!(build_txn(a, now()).unwrap().projects, vec!["japan-trip/work"]);
    }

    #[test]
    fn date_flag_overrides_now_and_sets_state() {
        let mut a = add_args();
        a.date = Some("2026-07-15".into()); // date-only → that day at local midnight
        a.cleared = true;
        let t = build_txn(a, now()).unwrap();
        assert_eq!(t.state, State::Cleared);
        assert_ne!(t.date, Some(now()));
        assert_eq!(t.date.unwrap().format("%Y-%m-%d").to_string(), "2026-07-15");
    }

    #[test]
    fn rejects_bracket_in_account() {
        let mut a = add_args();
        a.account = "ca[sh".into();
        assert!(build_txn(a, now()).is_err());
    }

    #[test]
    fn rejects_invalid_date() {
        let mut a = add_args();
        a.date = Some("18/07/2026".into());
        assert!(build_txn(a, now()).is_err());
    }

    #[test]
    fn long_entry_wraps_when_added() {
        // Many tags push past 66 cols → block form, every line within budget.
        let mut a = add_args();
        a.memo = vec!["latte".into()];
        a.project = vec!["japan-trip/leisure".into(), "work".into(), "reimbursable".into()];
        let entry = finance::format_entry(&build_txn(a, now()).unwrap());
        assert!(entry.contains('\n'), "should wrap: {entry}");
        assert!(entry.lines().all(|l| l.chars().count() <= 66), "over 66: {entry}");
    }
}
