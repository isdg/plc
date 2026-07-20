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

use std::fmt::Write as _;

use chrono::{DateTime, Datelike, FixedOffset, Local, LocalResult, NaiveDate, TimeZone};
use clap::{Args, Subcommand};
use plc_core::calendar::{self, MonthStats, YearStats};
use plc_core::finance::{self, Filter, Kind, Measure, State, Transaction};

use crate::cmd::calview;
use crate::config::Palace;
use crate::note;
use crate::settings::Settings;

/// The vault's default currency: `$PLC_CURRENCY` if set, else the `.plc/config`
/// `currency`, else `EUR`. (`fin add --cur` overrides this per transaction.)
fn resolved_currency(palace: &Palace) -> String {
    match std::env::var("PLC_CURRENCY") {
        Ok(c) if !c.trim().is_empty() => c.trim().to_uppercase(),
        _ => Settings::load(palace.root())
            .currency
            .unwrap_or_else(|| "EUR".to_string()),
    }
}

/// Heatmap-glyph meanings for the money scale (fixed buckets, currency units).
const MONEY_LEGEND: &str = "·  empty   ░ <5   ▒ <20   ▓ <50   █ ≥50";

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
    /// Short balance snapshot: net worth, account balances, recent transactions.
    #[command(alias = "bal")]
    Balance(BalanceArgs),
    /// Verify balance assertions (and, with --strict, undeclared names).
    Check(CheckArgs),
    /// Calendar heatmap / plot of daily spend, à la `plc stat`.
    Stat(FinStatArgs),
    /// Reformat every ledger file in place (canonical spacing / wrapping).
    Fmt(FmtArgs),
}

#[derive(Args)]
pub struct FmtArgs {
    /// Report which files would change, without writing them.
    #[arg(long = "check")]
    check: bool,
}

#[derive(Args)]
pub struct FinStatArgs {
    /// Limit to transactions whose account/category/tag/memo contains a PATTERN.
    #[arg(value_name = "PATTERN")]
    patterns: Vec<String>,
    /// Scope: `month` or `year`.
    #[arg(long = "type", value_name = "SCOPE", default_value = "month")]
    scope: String,
    /// Month 1-12 (defaults to the current month).
    #[arg(short = 'm', long = "month")]
    month: Option<String>,
    /// Year; 2-digit (25 → 2025) or 4-digit (defaults to the current year).
    #[arg(short = 'y', long = "year")]
    year: Option<String>,
    /// Year layout, `year` scope only: `git` (GitHub-style) or `tab`.
    #[arg(long = "layout", value_name = "LAYOUT", default_value = "git")]
    layout: String,
    /// Replace the heatmap with an ASCII line chart.
    #[arg(short = 'p', long = "plot")]
    plot: bool,
    /// What to measure per day: `expense` (default), `income`, or `net`.
    #[arg(long = "of", value_name = "MEASURE", default_value = "expense")]
    of: String,
    /// Only cleared (`*`) transactions.
    #[arg(long = "cleared", conflicts_with = "pending")]
    cleared: bool,
    /// Only pending (`!`) transactions.
    #[arg(long = "pending")]
    pending: bool,
}

#[derive(Args)]
pub struct CheckArgs {
    /// Also flag accounts/categories/commodities used but never declared
    /// (`account NAME` / `category NAME` / `commodity CODE` directive lines).
    #[arg(long = "strict")]
    strict: bool,
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
pub struct BalanceArgs {
    /// Same filtering as `report`/`reg` (PATTERN, --cleared/--pending, dates).
    #[command(flatten)]
    filter: ReportArgs,
    /// How many recent transactions to list (default 5).
    #[arg(short = 'n', long = "recent", value_name = "N", default_value = "5")]
    recent: usize,
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
    /// Split the amount across categories: `--split CAT=AMOUNT` (repeatable).
    /// The legs must sum to AMOUNT. Conflicts with --category/--to.
    #[arg(long = "split", value_name = "CAT=AMOUNT", conflicts_with_all = ["category", "to"])]
    split: Vec<String>,
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
    /// Assert the account's balance after this transaction (for reconciliation).
    #[arg(long = "assert", value_name = "BALANCE", allow_hyphen_values = true)]
    assert: Option<String>,
}

pub fn run(palace: &Palace, args: FinArgs) -> Result<String, String> {
    match args.cmd {
        None => seed_today(palace),
        Some(FinCmd::Add(add_args)) => add(palace, add_args),
        Some(FinCmd::Report(report_args)) => report(palace, report_args),
        Some(FinCmd::Reg(report_args)) => reg(palace, report_args),
        Some(FinCmd::Balance(balance_args)) => balance(palace, balance_args),
        Some(FinCmd::Check(check_args)) => {
            let cur = resolved_currency(palace);
            let root = palace.root().join("notes/management/daily");
            finance::check(&root, &cur, check_args.strict)
        }
        Some(FinCmd::Stat(stat_args)) => stat(palace, stat_args),
        Some(FinCmd::Fmt(fmt_args)) => {
            let cur = resolved_currency(palace);
            let root = palace.root().join("notes/management/daily");
            finance::fmt(&root, &cur, fmt_args.check)
        }
    }
}

/// `plc fin stat`: `plc stat`'s calendar/plot/stats visuals over daily spend.
fn stat(palace: &Palace, args: FinStatArgs) -> Result<String, String> {
    let today = Local::now().date_naive();
    let (y, m) = resolve_ym(&args, today)?;
    let measure = match args.of.as_str() {
        "expense" => Measure::Expense,
        "income" => Measure::Income,
        "net" => Measure::Net,
        o => return Err(format!("fin stat: unknown --of: {o} (expected expense|income|net)")),
    };
    let unit = match measure {
        Measure::Expense => "spend",
        Measure::Income => "income",
        Measure::Net => "net",
    };
    let state = if args.cleared {
        Some(State::Cleared)
    } else if args.pending {
        Some(State::Pending)
    } else {
        None
    };
    let filter = Filter {
        state,
        patterns: args.patterns.iter().map(|p| p.to_lowercase()).collect(),
        ..Filter::default()
    };
    let cur = resolved_currency(palace);
    let root = palace.root().join("notes/management/daily");
    let money = |minor| calendar::fmt_money(minor, &cur);

    match args.scope.as_str() {
        "month" => {
            let vals = finance::daily_spend(&root, &cur, &filter, y, Some(m), measure)?;
            let cutoff = (y == today.year() && m == today.month()).then_some(today.day());
            let st = calendar::month_stats(&vals, cutoff);
            let mut out = if args.plot {
                calview::plot_month(y, m, &vals, unit, &money)
            } else {
                calview::month_grid(y, m, &vals, calendar::money_symbol, MONEY_LEGEND)
            };
            push_month_spend(&mut out, &st, y, m, unit, &cur);
            Ok(out)
        }
        "year" => {
            let vals = finance::daily_spend(&root, &cur, &filter, y, None, measure)?;
            let cutoff = (y == today.year()).then_some(today.ordinal());
            let st = calendar::year_stats(&vals, y, cutoff);
            let mut out = if args.plot {
                calview::plot_year(y, &vals, unit, &money)
            } else {
                match args.layout.as_str() {
                    "git" => calview::year_git(y, &vals, calendar::money_symbol, MONEY_LEGEND),
                    "tab" => calview::year_tab(y, &vals, today, calendar::money_symbol, MONEY_LEGEND, &money),
                    o => return Err(format!("fin stat: unknown layout: {o} (expected git|tab)")),
                }
            };
            push_year_spend(&mut out, &st, y, unit, &cur);
            Ok(out)
        }
        o => Err(format!("fin stat: unknown type: {o} (expected month|year)")),
    }
}

/// Resolve `(year, month)` for `fin stat` from `-m/-y` (else today). Positional
/// args are a pattern filter, not a date, so they play no part here.
fn resolve_ym(args: &FinStatArgs, today: NaiveDate) -> Result<(i32, u32), String> {
    let m = match &args.month {
        Some(s) => s.trim().parse::<u32>().map_err(|_| format!("fin stat: invalid month: {s}"))?,
        None => today.month(),
    };
    let y = match &args.year {
        Some(s) => {
            let t = s.trim();
            let n: i32 = t.parse().map_err(|_| format!("fin stat: invalid year: {s}"))?;
            if t.len() == 2 && t.bytes().all(|b| b.is_ascii_digit()) { 2000 + n } else { n }
        }
        None => today.year(),
    };
    if !(1..=12).contains(&m) {
        return Err(format!("fin stat: invalid month: {m}"));
    }
    Ok((y, m))
}

/// The month spend Stats block (money-formatted; reworded from `plc stat`).
fn push_month_spend(out: &mut String, st: &MonthStats, y: i32, m: u32, unit: &str, cur: &str) {
    out.push_str("\n     ── Stats ─────────────────────────\n");
    let _ = writeln!(out, "     Days w/ {unit:<6}: {} / {}   ({}%)", st.days_written, st.last_day, st.pct);
    let _ = writeln!(out, "     Total        : {}", calendar::fmt_money(st.total, cur));
    if st.days_written > 0 {
        let avg = st.total / st.days_written as u64;
        let _ = writeln!(out, "     Avg / day    : {}", calendar::fmt_money(avg, cur));
    }
    let _ = writeln!(out, "     Longest run  : {} days", st.longest_run);
    let _ = writeln!(out, "     Current run  : {} days", st.current_run);
    if st.best_day > 0 {
        let mon = NaiveDate::from_ymd_opt(y, m, st.best_day).expect("best_day in-month").format("%b");
        let _ = writeln!(out, "     Biggest day  : {} {}   ({})", mon, st.best_day, calendar::fmt_money(st.best_size, cur));
    }
}

/// The year spend Stats block (money-formatted; reworded from `plc stat`).
fn push_year_spend(out: &mut String, st: &YearStats, y: i32, unit: &str, cur: &str) {
    out.push_str("\n  ── Year stats ───────────────────────────\n");
    let _ = writeln!(out, "  Days w/ {unit:<6}: {} / {}   ({}%)", st.days_written, st.total_days, st.pct);
    let _ = writeln!(out, "  Total        : {}", calendar::fmt_money(st.total, cur));
    if st.days_written > 0 {
        let avg = st.total / st.days_written as u64;
        let _ = writeln!(out, "  Avg / day    : {}", calendar::fmt_money(avg, cur));
    }
    let _ = writeln!(out, "  Longest run  : {} days", st.longest_run);
    let _ = writeln!(out, "  Current run  : {} days", st.current_run);
    if st.best_month > 0 {
        let name = NaiveDate::from_ymd_opt(y, st.best_month, 1).unwrap().format("%B").to_string();
        let _ = writeln!(out, "  Biggest month: {name:<9} ({})", calendar::fmt_money(st.best_month_total, cur));
    }
    if st.best_day_month > 0 {
        let mon = NaiveDate::from_ymd_opt(y, st.best_day_month, st.best_day_dom).unwrap().format("%b");
        let _ = writeln!(out, "  Biggest day  : {} {}   ({})", mon, st.best_day_dom, calendar::fmt_money(st.best_size, cur));
    }
}

/// `plc fin report`: aggregate the matching `+ledger` transactions.
fn report(palace: &Palace, args: ReportArgs) -> Result<String, String> {
    let filter = build_filter(&args)?;
    let root = palace.root().join("notes/management/daily");
    finance::report(&root, &resolved_currency(palace), &filter)
}

/// `plc fin reg`: chronological register of the matching transactions.
fn reg(palace: &Palace, args: ReportArgs) -> Result<String, String> {
    let filter = build_filter(&args)?;
    let root = palace.root().join("notes/management/daily");
    finance::register(&root, &resolved_currency(palace), &filter)
}

/// `plc fin balance` (alias `bal`): short net-worth + account snapshot with the
/// most recent transactions.
fn balance(palace: &Palace, args: BalanceArgs) -> Result<String, String> {
    let filter = build_filter(&args.filter)?;
    let root = palace.root().join("notes/management/daily");
    finance::balance(&root, &resolved_currency(palace), &filter, args.recent)
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

/// Ledger location `(subdir, filename)` for a given day under the daily tree.
fn ledger_location(date: NaiveDate) -> (String, String) {
    (
        format!("notes/management/daily/{:04}/{:02}", date.year(), date.month()),
        format!("{:04}-{:02}-{:02}+ledger.md", date.year(), date.month(), date.day()),
    )
}

/// Bare `plc fin`: seed today's ledger (if new) and print its path.
fn seed_today(palace: &Palace) -> Result<String, String> {
    let (subdir, filename) = ledger_location(Local::now().date_naive());
    note::ensure_note(palace.root(), &subdir, &filename, "ledger", None, note::SIGNATURE)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("fin: {e}"))
}

/// `plc fin add`: build the transaction and append it to its day's ledger — the
/// transaction's own date (from `--date`, else today), so a back-dated entry
/// lands in the correct `YYYY-MM-DD+ledger.md`, not today's.
fn add(palace: &Palace, args: AddArgs) -> Result<String, String> {
    let default_cur = resolved_currency(palace);
    let txn = build_txn(args, Local::now().fixed_offset(), &default_cur)?;
    let day = txn.date.map_or_else(|| Local::now().date_naive(), |d| d.date_naive());
    let entry = finance::format_entry(&txn);
    let (subdir, filename) = ledger_location(day);
    note::append_line(palace.root(), &subdir, &filename, "ledger", &entry)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("fin: {e}"))
}

/// Build a [`Transaction`] from `add` args. `now` is the default timestamp used
/// when `--date` is absent (injected so tests are deterministic).
fn build_txn(
    args: AddArgs,
    now: DateTime<FixedOffset>,
    default_currency: &str,
) -> Result<Transaction, String> {
    let amount = finance::amount_to_minor(&args.amount)
        .ok_or_else(|| format!("fin: invalid amount: {}", args.amount))?;
    let account = clean_link("account", &args.account)?;
    let currency = args
        .currency
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| default_currency.to_string());

    let (kind, other) = match (args.to, args.category, args.income) {
        (Some(dest), _, _) => (Kind::Transfer, Some(clean_link("account", &dest)?)),
        (None, cat, income) => {
            let kind = if income { Kind::Income } else { Kind::Expense };
            (kind, cat.map(|c| clean_link("category", &c)).transpose()?)
        }
    };

    // Split legs (if any) distribute `amount` across categories; they must sum
    // to it, and the single `other` category is dropped.
    let split = parse_splits(&args.split)?;
    if !split.is_empty() {
        let sum: i64 = split.iter().map(|(_, a)| a).sum();
        if sum != amount {
            return Err(format!(
                "fin: split legs sum to {sum} minor units, not the stated total {amount}"
            ));
        }
    }
    let other = if split.is_empty() { other } else { None };

    let projects = args
        .project
        .iter()
        .map(|p| clean_link("project", p).map(|p| finance::normalize_name(&p)))
        .collect::<Result<Vec<_>, _>>()?;

    let assert = match args.assert.as_deref() {
        Some(s) => Some(parse_balance(s)?),
        None => None,
    };

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
        assert,
        date,
        state,
        projects,
        split,
        memo: args.memo.join(" "),
    })
}

/// Parse `--split CAT=AMOUNT` args into `(normalized category, minor units)`.
fn parse_splits(args: &[String]) -> Result<Vec<(String, i64)>, String> {
    args.iter()
        .map(|s| {
            let (cat, amt) = s
                .split_once('=')
                .ok_or_else(|| format!("fin: --split wants CAT=AMOUNT, got: {s}"))?;
            let cat = finance::normalize_name(&clean_link("split category", cat)?);
            let amount = finance::amount_to_minor(amt)
                .ok_or_else(|| format!("fin: invalid split amount: {amt}"))?;
            Ok((cat, amount))
        })
        .collect()
}

/// Parse a signed balance for `--assert` into minor units.
fn parse_balance(s: &str) -> Result<i64, String> {
    let s = s.trim();
    let (neg, mag) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let minor = finance::amount_to_minor(mag)
        .ok_or_else(|| format!("fin: invalid assert balance: {s}"))?;
    Ok(if neg { -minor } else { minor })
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
            split: vec![],
            to: None,
            income: false,
            currency: Some("EUR".into()),
            project: vec![],
            date: None,
            cleared: false,
            pending: false,
            assert: None,
        }
    }

    /// A fixed "now" so `build_txn`'s default stamp is deterministic in tests.
    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_str("2026-07-19 11:28:22 +0200", finance::TIMESTAMP_FMT).unwrap()
    }

    #[test]
    fn maps_expense_args() {
        let t = build_txn(add_args(), now(), "EUR").unwrap();
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
        let t = build_txn(a, now(), "EUR").unwrap();
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
        let t = build_txn(a, now(), "EUR").unwrap();
        assert_eq!(t.kind, Kind::Transfer);
        assert_eq!((t.account.as_str(), t.other.as_deref()), ("cash", Some("checking")));
    }

    #[test]
    fn projects_normalized_slash_preserved() {
        let mut a = add_args();
        a.project = vec!["Japan-Trip/Work".into()];
        assert_eq!(build_txn(a, now(), "EUR").unwrap().projects, vec!["japan-trip/work"]);
    }

    #[test]
    fn date_flag_overrides_now_and_sets_state() {
        let mut a = add_args();
        a.date = Some("2026-07-15".into()); // date-only → that day at local midnight
        a.cleared = true;
        let t = build_txn(a, now(), "EUR").unwrap();
        assert_eq!(t.state, State::Cleared);
        assert_ne!(t.date, Some(now()));
        assert_eq!(t.date.unwrap().format("%Y-%m-%d").to_string(), "2026-07-15");
    }

    #[test]
    fn rejects_bracket_in_account() {
        let mut a = add_args();
        a.account = "ca[sh".into();
        assert!(build_txn(a, now(), "EUR").is_err());
    }

    #[test]
    fn split_legs_distribute_and_must_sum() {
        let mut a = add_args();
        a.amount = "90".into();
        a.category = None;
        a.memo = vec!["Costco".into()];
        a.split = vec!["food=60".into(), "household=25".into(), "tax=5".into()];
        let t = build_txn(a, now(), "EUR").unwrap();
        assert_eq!(t.amount, 9000);
        assert_eq!(t.other, None);
        assert_eq!(t.split, vec![("food".into(), 6000), ("household".into(), 2500), ("tax".into(), 500)]);

        // Legs that don't sum to the total are rejected.
        let mut bad = add_args();
        bad.amount = "90".into();
        bad.category = None;
        bad.split = vec!["food=60".into(), "tax=5".into()];
        assert!(build_txn(bad, now(), "EUR").is_err());
    }

    #[test]
    fn assert_flag_parses_signed_balance() {
        let mut a = add_args();
        a.assert = Some("-12.00".into());
        assert_eq!(build_txn(a, now(), "EUR").unwrap().assert, Some(-1200));
    }

    #[test]
    fn rejects_invalid_date() {
        let mut a = add_args();
        a.date = Some("18/07/2026".into());
        assert!(build_txn(a, now(), "EUR").is_err());
    }

    #[test]
    fn long_entry_wraps_when_added() {
        // Tags and memo drop to continuation lines (the head keeps the
        // accounting); every continuation line stays within the 79-col budget.
        let mut a = add_args();
        a.memo = vec!["latte".into()];
        a.project = vec!["japan-trip/leisure".into(), "work".into(), "reimbursable".into()];
        let entry = finance::format_entry(&build_txn(a, now(), "EUR").unwrap());
        assert!(entry.contains('\n'), "should wrap: {entry}");
        for line in entry.lines().skip(1) {
            assert!(line.chars().count() <= 79, "continuation over 79: {line}");
        }
    }
}
