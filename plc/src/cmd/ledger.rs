//! `plc ledger` — plain-text ledger tracking, kept beside your daily notes.
//!
//! Transactions live in a per-day ledger file next to the daily note but marked
//! by a `+ledger` filename postfix and tagged `[[ledger]]`:
//!
//! ```text
//! notes/management/daily/2026/07/2026-07-19+ledger.md
//! ```
//!
//! `plc ledger add` formats a transaction line and appends it to today's ledger in
//! one shot (seeding the header first if the file is new). A transaction line is:
//!
//! ```text
//! $ <±amount> [CUR]  @[[account]]  (#[[category]] | > @[[account2]])  [memo…]
//! ```
//!
//! `-` is an expense (outflow), `+` income (inflow), and `> @[[dest]]` a transfer;
//! accounts/categories stay `[[wikilinks]]` so the link/orphans engine still sees
//! them. See `plc_core::ledger` for the full grammar. Bare `plc ledger` just seeds
//! and prints today's ledger path so you can open it by hand.

use std::fmt::Write as _;

use chrono::{DateTime, Datelike, FixedOffset, Local, LocalResult, NaiveDate, TimeZone};
use clap::{Args, Subcommand};
use plc_core::calendar::{self, MonthStats, YearStats};
use plc_core::ledger::{self, Filter, Kind, Measure, State, Transaction};

use crate::cmd::calview;
use crate::config::Palace;
use crate::note;
use crate::settings::Settings;

/// The vault's default currency: `$PLC_CURRENCY` if set, else the `.plc/config`
/// `currency`, else `EUR`. (`ledger add --cur` overrides this per transaction.)
fn resolved_currency(palace: &Palace) -> String {
    currency_from(&Settings::load(palace.root()))
}

/// Currency from an already-loaded `Settings`: `$PLC_CURRENCY` > config > `EUR`.
fn currency_from(settings: &Settings) -> String {
    match std::env::var("PLC_CURRENCY") {
        Ok(c) if !c.trim().is_empty() => c.trim().to_uppercase(),
        _ => settings.currency.clone().unwrap_or_else(|| "EUR".to_string()),
    }
}

/// Which declared set `declare` operates on. Physical accounts and ephemeral
/// categories are the same essence (named ledger buckets), so one command with
/// a `--physical` / `--ephemeral` flag serves both.
#[derive(Clone, Copy)]
enum Decl {
    Account,
    Category,
}

impl Decl {
    fn label(self) -> &'static str {
        match self {
            Decl::Account => "account",
            Decl::Category => "category",
        }
    }
    fn plural(self) -> &'static str {
        match self {
            Decl::Account => "accounts",
            Decl::Category => "categories",
        }
    }
    fn sigil(self) -> char {
        match self {
            Decl::Account => '@',
            Decl::Category => '#',
        }
    }
    /// The `plc ledger declare` flag that selects this kind.
    fn flag(self) -> &'static str {
        match self {
            Decl::Account => "--physical",
            Decl::Category => "--ephemeral",
        }
    }
}

/// Heatmap-glyph meanings for the money scale (fixed buckets, currency units).
const MONEY_LEGEND: &str = "·  empty   ░ <5   ▒ <20   ▓ <50   █ ≥50";

#[derive(Args)]
pub struct LedgerArgs {
    #[command(subcommand)]
    cmd: Option<LedgerCmd>,
}

#[derive(Subcommand)]
enum LedgerCmd {
    /// Append a transaction to today's ledger.
    Add(AddArgs),
    /// Edit a transaction by its ^id (a unique prefix, git-style). Bare = print
    /// its file path:line for an editor; with flags = apply the changes in place.
    Edit(EditArgs),
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
    Stat(LedgerStatArgs),
    /// Reformat every ledger file in place (canonical spacing / wrapping).
    Fmt(FmtArgs),
    /// Declare/list the known accounts (`--physical`) and categories
    /// (`--ephemeral`). Bare = list both; NAME(s) add; `-r` remove; `--import`.
    Declare(DeclareArgs),
    /// Show the most recently added transactions (recent-activity log).
    Last(LastArgs),
    /// Remove the last added transaction from its ledger and the log.
    Undo,
}

#[derive(Args)]
pub struct DeclareArgs {
    /// Names to declare (or, with `-r`, remove). Omit to list.
    #[arg(value_name = "NAME")]
    names: Vec<String>,
    /// Operate on physical accounts (`@`).
    #[arg(long = "physical", conflicts_with = "ephemeral")]
    physical: bool,
    /// Operate on ephemeral categories (`#`).
    #[arg(long = "ephemeral")]
    ephemeral: bool,
    /// Remove the named entries instead of adding them.
    #[arg(short = 'r', long = "rm")]
    rm: bool,
    /// Seed the set from every name already used across the ledgers.
    #[arg(long = "import", conflicts_with = "rm")]
    import: bool,
}

#[derive(Args)]
pub struct LastArgs {
    /// How many recent transactions to show (default 10).
    #[arg(short = 'n', long = "recent", value_name = "N", default_value = "10")]
    recent: usize,
}

#[derive(Args)]
pub struct FmtArgs {
    /// Report which files would change, without writing them.
    #[arg(long = "check")]
    check: bool,
}

#[derive(Args)]
pub struct LedgerStatArgs {
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
    /// Account the money moves through. Required unless `-T` supplies it.
    #[arg(short = 'a', long = "account", value_name = "ACCOUNT")]
    account: Option<String>,
    /// Symbolic transaction shape, an alternative to `-a`/`-c`/`--to`/`-i`/
    /// `--assert`: `"revolut -> food/out"` (expense), `"revolut -> cash"`
    /// (transfer, when the target is an account), `"revolut <- salary"` (income),
    /// `"revolut = 2300"` (assertion). Prefix a name with `@`/`#` to force
    /// account/category; otherwise a name is an account if it's declared.
    #[arg(short = 'T', long = "txn", value_name = "SPEC",
          conflicts_with_all = ["account", "category", "to", "income", "assert", "split"])]
    txn: Option<String>,
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
    /// Declare any new account/category used here on the fly, instead of being
    /// rejected by the typo guard (when a declared set exists).
    #[arg(short = 'n', long = "new")]
    new: bool,
    /// Assert the account's balance after this transaction (for reconciliation).
    #[arg(long = "assert", value_name = "BALANCE", allow_hyphen_values = true)]
    assert: Option<String>,
}

/// `plc ledger edit <ID>`: change one transaction, found by its `^id` (a unique
/// prefix suffices). With no field flags it just prints the transaction's
/// `path:line` (for an editor to open); any field flag applies the change in
/// place, keeping the frozen id. A `--date` that lands on another day moves the
/// entry to that day's ledger file.
#[derive(Args)]
pub struct EditArgs {
    /// The transaction's `^id` — a unique prefix is enough (git-style).
    #[arg(value_name = "ID")]
    id: String,
    /// New amount in the major unit (arithmetic allowed); keeps the direction.
    #[arg(long = "amount", value_name = "AMOUNT")]
    amount: Option<String>,
    /// Replace the memo (empty string clears it).
    #[arg(long = "memo", value_name = "MEMO")]
    memo: Option<String>,
    /// Change the account the money moves through.
    #[arg(short = 'a', long = "account", value_name = "ACCOUNT")]
    account: Option<String>,
    /// Set the category (expense/income); clears any transfer/split.
    #[arg(short = 'c', long = "category", value_name = "CATEGORY", conflicts_with = "to")]
    category: Option<String>,
    /// Make it a transfer to this account instead.
    #[arg(long = "to", value_name = "ACCOUNT")]
    to: Option<String>,
    /// Flip the direction to income (inflow).
    #[arg(short = 'i', long = "income", conflicts_with_all = ["expense", "to"])]
    income: bool,
    /// Flip the direction to expense (outflow).
    #[arg(long = "expense", conflicts_with = "to")]
    expense: bool,
    /// Change the currency ISO code.
    #[arg(long = "cur", value_name = "CUR")]
    currency: Option<String>,
    /// Replace the project tags (repeatable).
    #[arg(short = 'p', long = "project", value_name = "PROJECT")]
    project: Vec<String>,
    /// Drop all project tags.
    #[arg(long = "no-projects", conflicts_with = "project")]
    no_projects: bool,
    /// Change the date (YYYY-MM-DD or a full timestamp).
    #[arg(short = 'd', long = "date", value_name = "DATE")]
    date: Option<String>,
    /// Mark cleared (`*`).
    #[arg(long = "cleared", conflicts_with_all = ["pending", "uncleared"])]
    cleared: bool,
    /// Mark pending (`!`).
    #[arg(long = "pending", conflicts_with = "uncleared")]
    pending: bool,
    /// Clear the reconciliation state (uncleared).
    #[arg(long = "uncleared")]
    uncleared: bool,
    /// Set the balance assertion.
    #[arg(long = "assert", value_name = "BALANCE", allow_hyphen_values = true, conflicts_with = "no_assert")]
    assert: Option<String>,
    /// Remove the balance assertion.
    #[arg(long = "no-assert")]
    no_assert: bool,
}

pub fn run(palace: &Palace, args: LedgerArgs) -> Result<String, String> {
    match args.cmd {
        None => seed_today(palace),
        Some(LedgerCmd::Add(add_args)) => add(palace, add_args),
        Some(LedgerCmd::Edit(edit_args)) => edit(palace, edit_args),
        Some(LedgerCmd::Report(report_args)) => report(palace, report_args),
        Some(LedgerCmd::Reg(report_args)) => reg(palace, report_args),
        Some(LedgerCmd::Balance(balance_args)) => balance(palace, balance_args),
        Some(LedgerCmd::Check(check_args)) => {
            let settings = Settings::load(palace.root());
            let cur = currency_from(&settings);
            let root = palace.root().join("notes/management/daily");
            ledger::check(&root, &cur, check_args.strict, &settings.accounts, &settings.categories)
        }
        Some(LedgerCmd::Declare(declare_args)) => declare_cmd(palace, declare_args),
        Some(LedgerCmd::Last(last_args)) => last_log(palace, last_args),
        Some(LedgerCmd::Undo) => undo(palace),
        Some(LedgerCmd::Stat(stat_args)) => stat(palace, stat_args),
        Some(LedgerCmd::Fmt(fmt_args)) => {
            let cur = resolved_currency(palace);
            let root = palace.root().join("notes/management/daily");
            ledger::fmt(&root, &cur, fmt_args.check)
        }
    }
}

/// `plc ledger stat`: `plc stat`'s calendar/plot/stats visuals over daily spend.
fn stat(palace: &Palace, args: LedgerStatArgs) -> Result<String, String> {
    let today = Local::now().date_naive();
    let (y, m) = resolve_ym(&args, today)?;
    let measure = match args.of.as_str() {
        "expense" => Measure::Expense,
        "income" => Measure::Income,
        "net" => Measure::Net,
        o => return Err(format!("ledger stat: unknown --of: {o} (expected expense|income|net)")),
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
            let vals = ledger::daily_spend(&root, &cur, &filter, y, Some(m), measure)?;
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
            let vals = ledger::daily_spend(&root, &cur, &filter, y, None, measure)?;
            let cutoff = (y == today.year()).then_some(today.ordinal());
            let st = calendar::year_stats(&vals, y, cutoff);
            let mut out = if args.plot {
                calview::plot_year(y, &vals, unit, &money)
            } else {
                match args.layout.as_str() {
                    "git" => calview::year_git(y, &vals, calendar::money_symbol, MONEY_LEGEND),
                    "tab" => calview::year_tab(y, &vals, today, calendar::money_symbol, MONEY_LEGEND, &money),
                    o => return Err(format!("ledger stat: unknown layout: {o} (expected git|tab)")),
                }
            };
            push_year_spend(&mut out, &st, y, unit, &cur);
            Ok(out)
        }
        o => Err(format!("ledger stat: unknown type: {o} (expected month|year)")),
    }
}

/// Resolve `(year, month)` for `fin stat` from `-m/-y` (else today). Positional
/// args are a pattern filter, not a date, so they play no part here.
fn resolve_ym(args: &LedgerStatArgs, today: NaiveDate) -> Result<(i32, u32), String> {
    let m = match &args.month {
        Some(s) => s.trim().parse::<u32>().map_err(|_| format!("ledger stat: invalid month: {s}"))?,
        None => today.month(),
    };
    let y = match &args.year {
        Some(s) => {
            let t = s.trim();
            let n: i32 = t.parse().map_err(|_| format!("ledger stat: invalid year: {s}"))?;
            if t.len() == 2 && t.bytes().all(|b| b.is_ascii_digit()) { 2000 + n } else { n }
        }
        None => today.year(),
    };
    if !(1..=12).contains(&m) {
        return Err(format!("ledger stat: invalid month: {m}"));
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

/// `plc ledger report`: aggregate the matching `+ledger` transactions.
fn report(palace: &Palace, args: ReportArgs) -> Result<String, String> {
    let filter = build_filter(&args)?;
    let root = palace.root().join("notes/management/daily");
    ledger::report(&root, &resolved_currency(palace), &filter)
}

/// `plc ledger reg`: chronological register of the matching transactions.
fn reg(palace: &Palace, args: ReportArgs) -> Result<String, String> {
    let filter = build_filter(&args)?;
    let root = palace.root().join("notes/management/daily");
    ledger::register(&root, &resolved_currency(palace), &filter)
}

/// `plc ledger balance` (alias `bal`): short net-worth + account snapshot with the
/// most recent transactions.
fn balance(palace: &Palace, args: BalanceArgs) -> Result<String, String> {
    let filter = build_filter(&args.filter)?;
    let root = palace.root().join("notes/management/daily");
    ledger::balance(&root, &resolved_currency(palace), &filter, args.recent)
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
            .map_err(|_| format!("ledger: invalid month (want YYYY-MM): {m}"))?;
        let (y, mo) = (first.year(), first.month());
        let (ny, nm) = if mo == 12 { (y + 1, 1) } else { (y, mo + 1) };
        let last = NaiveDate::from_ymd_opt(ny, nm, 1)
            .and_then(|d| d.pred_opt())
            .ok_or_else(|| format!("ledger: invalid month: {m}"))?;
        return Ok((Some(first), Some(last)));
    }
    let parse = |o: &Option<String>| match o {
        Some(s) => NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
            .map(Some)
            .map_err(|_| format!("ledger: invalid date (want YYYY-MM-DD): {s}")),
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

/// Recent-transaction log file inside the `.plc` state dir.
const LOG: &str = "last-transactions";
/// Cap the log at this many records so it can't grow without bound.
const LOG_CAP: usize = 200;

/// One logged add: the ledger file (vault-relative) and its rendered entry block.
struct LogRecord {
    path: String,
    entry: String,
}

fn log_file(palace: &Palace) -> std::path::PathBuf {
    palace.state_dir().join(LOG)
}

/// Rebuild `.plc/last-transactions` from the ledgers and return the records
/// (oldest→newest). Always current and self-creating: this is the single point
/// that keeps the cache in sync, called on every add / last / undo so the file
/// never goes stale or missing. `ledger::recent_entries` returns paths relative
/// to the daily tree; we store them vault-relative.
fn sync_log(palace: &Palace) -> Result<Vec<LogRecord>, String> {
    let daily = "notes/management/daily";
    let entries = ledger::recent_entries(&palace.root().join(daily), &resolved_currency(palace), LOG_CAP)?;
    let records: Vec<LogRecord> = entries
        .into_iter()
        .map(|(rel, entry)| LogRecord { path: format!("{daily}/{rel}"), entry })
        .collect();
    write_log(palace, &records)?;
    Ok(records)
}

fn render_log(records: &[LogRecord]) -> String {
    let mut out = String::new();
    for r in records {
        out.push_str(&format!("==== {}\n{}\n", r.path, r.entry));
    }
    out
}

fn write_log(palace: &Palace, records: &[LogRecord]) -> Result<(), String> {
    std::fs::create_dir_all(palace.state_dir()).map_err(|e| format!("ledger: {e}"))?;
    std::fs::write(log_file(palace), render_log(records)).map_err(|e| format!("ledger: {e}"))
}

/// Remove the last occurrence of the `entry` block from `content` (plus one
/// adjacent newline), returning the cleaned file text. `None` if not found.
fn remove_last_block(content: &str, entry: &str) -> Option<String> {
    let idx = content.rfind(entry)?;
    let mut end = idx + entry.len();
    if content[end..].starts_with('\n') {
        end += 1;
    }
    let joined = format!("{}{}", &content[..idx], &content[end..]);
    Some(format!("{}\n", joined.trim_end()))
}

/// `plc ledger last`: the most recent transactions, newest first, from the
/// always-current cache (rebuilt from the ledgers, so it covers all history).
fn last_log(palace: &Palace, args: LastArgs) -> Result<String, String> {
    let records = sync_log(palace)?;
    if records.is_empty() {
        return Ok("\n  (no transactions found)".to_string());
    }
    let shown = args.recent.min(records.len());
    let mut out = vec![String::new(), format!("  Recent — {shown} of {} transaction(s)", records.len()), String::new()];
    for r in records.iter().rev().take(args.recent) {
        out.extend(r.entry.lines().map(|l| format!("  {l}")));
        out.push(String::new());
    }
    Ok(out.join("\n").trim_end().to_string())
}

/// `plc ledger undo`: remove the most recent transaction from its ledger and refresh
/// the cache. Refuses if the recorded block is no longer in the file (edited).
fn undo(palace: &Palace) -> Result<String, String> {
    let records = sync_log(palace)?;
    let last = records.last().ok_or_else(|| "ledger undo: nothing to undo".to_string())?;
    let file = palace.root().join(&last.path);
    let content = std::fs::read_to_string(&file)
        .map_err(|e| format!("ledger undo: cannot read {}: {e}", last.path))?;
    let kept = remove_last_block(&content, &last.entry).ok_or_else(|| {
        format!("ledger undo: the entry is no longer in {} (edited since?) — not undoing", last.path)
    })?;
    std::fs::write(&file, kept).map_err(|e| format!("ledger undo: {e}"))?;
    let removed = last.entry.clone();
    let path = last.path.clone();
    sync_log(palace)?; // refresh after the removal
    Ok(format!("undid (removed from {path}):\n{removed}"))
}

/// Bare `plc ledger`: seed today's ledger (if new) and print its path.
fn seed_today(palace: &Palace) -> Result<String, String> {
    let (subdir, filename) = ledger_location(Local::now().date_naive());
    note::ensure_note(palace.root(), &subdir, &filename, "ledger", None, note::SIGNATURE)
        .map(|p| p.display().to_string())
        .map_err(|e| format!("ledger: {e}"))
}

/// `plc ledger add`: build the transaction and append it to its day's ledger — the
/// transaction's own date (from `--date`, else today), so a back-dated entry
/// lands in the correct `YYYY-MM-DD+ledger.md`, not today's.
fn add(palace: &Palace, args: AddArgs) -> Result<String, String> {
    let mut settings = Settings::load(palace.root());
    let default_cur = currency_from(&settings);
    let declare_new = args.new;
    let txn = build_txn(args, Local::now().fixed_offset(), &default_cur, &settings.accounts)?;

    // Typo guard: reject an account/category not in a non-empty declared set,
    // unless `-n` declares it now. Empty sets → no check (fresh-vault default).
    guard_declared(palace, &mut settings, &txn, declare_new)?;

    let day = txn.date.map_or_else(|| Local::now().date_naive(), |d| d.date_naive());
    let entry = ledger::format_entry(&txn);
    let (subdir, filename) = ledger_location(day);
    let path = note::append_line(palace.root(), &subdir, &filename, "ledger", &entry)
        .map_err(|e| format!("ledger: {e}"))?;
    let _ = sync_log(palace); // keep the recent cache current (best-effort)
    Ok(path.display().to_string())
}

/// `plc ledger edit <ID>`: locate a transaction by its `^id` and either print its
/// `path:line` (no flags — for an editor to open) or apply the field edits in
/// place (keeping the frozen id). A day-changing `--date` moves the entry to the
/// right day's ledger file.
fn edit(palace: &Palace, args: EditArgs) -> Result<String, String> {
    let cur = currency_from(&Settings::load(palace.root()));
    let root = palace.root().join("notes/management/daily");
    let matches = ledger::find_by_id(&root, &cur, &args.id)?;
    if matches.is_empty() {
        return Err(format!("ledger edit: no transaction with id ^{}", args.id));
    }
    if matches.len() > 1 {
        let mut lines = vec![format!("ledger edit: ambiguous id ^{} — matches {}:", args.id, matches.len())];
        lines.extend(matches.iter().map(|(p, t)| format!("  ^{}  {p}", t.id.as_deref().unwrap_or("?"))));
        return Err(lines.join("\n"));
    }
    let (relpath, old) = matches.into_iter().next().unwrap();
    let full_id = old.id.clone().ok_or_else(|| "ledger edit: matched transaction has no id".to_string())?;
    let oldpath = root.join(&relpath);

    // No field flags → locate mode: print `path:line` for the shell to open.
    if !has_edits(&args) {
        let line = head_line_number(&oldpath, &full_id)?;
        return Ok(format!("{}:{}", oldpath.display(), line));
    }

    let edited = apply_edits(old, &args)?;

    // A `--date` onto a different day moves the entry to that day's ledger file.
    let old_day = rel_file_day(&relpath);
    let new_day = edited.date.map(|d| d.date_naive());
    if let (Some(o), Some(n)) = (old_day, new_day) {
        if o != n {
            if !ledger::rewrite_txn(&oldpath, &cur, &full_id, None)? {
                return Err(format!("ledger edit: id ^{full_id} vanished from {relpath}"));
            }
            let (subdir, filename) = ledger_location(n);
            let entry = ledger::format_entry(&edited);
            let path = note::append_line(palace.root(), &subdir, &filename, "ledger", &entry)
                .map_err(|e| format!("ledger edit: {e}"))?;
            let _ = sync_log(palace);
            return Ok(path.display().to_string());
        }
    }

    // In place: rewrite the entry in its current file.
    if !ledger::rewrite_txn(&oldpath, &cur, &full_id, Some(&edited))? {
        return Err(format!("ledger edit: id ^{full_id} vanished from {relpath}"));
    }
    let _ = sync_log(palace);
    Ok(oldpath.display().to_string())
}

/// Whether any field-editing flag was passed (as opposed to a bare locate).
fn has_edits(a: &EditArgs) -> bool {
    a.amount.is_some()
        || a.memo.is_some()
        || a.account.is_some()
        || a.category.is_some()
        || a.to.is_some()
        || a.income
        || a.expense
        || a.currency.is_some()
        || !a.project.is_empty()
        || a.no_projects
        || a.date.is_some()
        || a.cleared
        || a.pending
        || a.uncleared
        || a.assert.is_some()
        || a.no_assert
}

/// Apply the requested field edits to `t`, returning the edited transaction. The
/// `^id` is left untouched (a frozen handle survives an edit).
fn apply_edits(mut t: Transaction, a: &EditArgs) -> Result<Transaction, String> {
    // A split's amount is the sum of its legs; changing it here would unbalance
    // the book, so refuse (edit the legs in the file, or undo and re-add).
    if !t.split.is_empty() && a.amount.is_some() {
        return Err("ledger edit: cannot change a split's amount — edit its legs in the file, or undo + re-add".to_string());
    }
    if let Some(s) = &a.amount {
        t.amount = ledger::eval_amount(s).ok_or_else(|| format!("ledger edit: invalid amount: {s}"))?;
    }
    if let Some(m) = &a.memo {
        t.memo = m.trim().to_string();
    }
    if let Some(acc) = &a.account {
        t.account = ledger::normalize_name(&clean_link("account", acc)?);
    }
    if let Some(c) = &a.currency {
        let c = c.trim().to_uppercase();
        if !c.is_empty() {
            t.currency = c;
        }
    }
    // Kind / counterpart: --to makes a transfer, --category an expense/income.
    if let Some(dest) = &a.to {
        t.kind = Kind::Transfer;
        t.other = Some(ledger::normalize_name(&clean_link("account", dest)?));
        t.split.clear();
    } else if let Some(cat) = &a.category {
        if matches!(t.kind, Kind::Transfer) {
            t.kind = Kind::Expense; // a category can't hang off a transfer
        }
        t.other = Some(ledger::normalize_name(&clean_link("category", cat)?));
        t.split.clear();
    }
    if a.income {
        t.kind = Kind::Income;
    } else if a.expense {
        t.kind = Kind::Expense;
    }
    if a.no_projects {
        t.projects.clear();
    } else if !a.project.is_empty() {
        t.projects = a
            .project
            .iter()
            .map(|p| clean_link("project", p).map(|p| ledger::normalize_name(&p)))
            .collect::<Result<Vec<_>, _>>()?;
    }
    if let Some(d) = &a.date {
        t.date = Some(parse_when(d)?);
    }
    if a.cleared {
        t.state = State::Cleared;
    } else if a.pending {
        t.state = State::Pending;
    } else if a.uncleared {
        t.state = State::Uncleared;
    }
    if a.no_assert {
        t.assert = None;
    } else if let Some(s) = &a.assert {
        t.assert = Some(parse_balance(s)?);
    }

    // A transaction must move between distinct buckets (mirrors `build_txn`).
    let clashes = t.other.as_deref() == Some(t.account.as_str()) || t.split.iter().any(|(c, _)| c == &t.account);
    if clashes {
        return Err(if matches!(t.kind, Kind::Transfer) {
            format!("ledger edit: source and destination are the same account (@{})", t.account)
        } else {
            format!("ledger edit: @{0} and #{0} are the same name — use distinct names", t.account)
        });
    }
    Ok(t)
}

/// The 1-based line number of the `$` head line carrying `^full_id` in `path`.
fn head_line_number(path: &std::path::Path, full_id: &str) -> Result<usize, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("ledger edit: {}: {e}", path.display()))?;
    let needle = format!("^{full_id}");
    content
        .lines()
        .position(|l| l.trim_start().starts_with('$') && l.contains(&needle))
        .map(|i| i + 1)
        .ok_or_else(|| format!("ledger edit: id ^{full_id} not found in {}", path.display()))
}

/// The day encoded in a ledger's `…/YYYY-MM-DD+ledger.md` relative path.
fn rel_file_day(rel: &str) -> Option<NaiveDate> {
    let base = rel.rsplit('/').next()?;
    NaiveDate::parse_from_str(base.get(..10)?, "%Y-%m-%d").ok()
}

/// The account(s) and category(ies) a transaction actually uses.
fn used_names(txn: &Transaction) -> (Vec<String>, Vec<String>) {
    let mut accts = vec![txn.account.clone()];
    let mut cats = Vec::new();
    match txn.kind {
        Kind::Transfer => accts.extend(txn.other.clone()),
        _ if !txn.split.is_empty() => cats.extend(txn.split.iter().map(|(c, _)| c.clone())),
        _ => cats.extend(txn.other.clone()),
    }
    (accts, cats)
}

/// Reject an account/category absent from a *non-empty* declared set. With
/// `declare_new`, add the unknown names to `.plc/config` and proceed instead.
fn guard_declared(
    palace: &Palace,
    settings: &mut Settings,
    txn: &Transaction,
    declare_new: bool,
) -> Result<(), String> {
    let (accts, cats) = used_names(txn);
    let missing = |used: &[String], declared: &[String]| -> Vec<String> {
        if declared.is_empty() {
            return Vec::new(); // no declarations yet → nothing to enforce
        }
        used.iter().filter(|n| !declared.contains(n)).cloned().collect()
    };
    let bad_accts = missing(&accts, &settings.accounts);
    let bad_cats = missing(&cats, &settings.categories);
    if bad_accts.is_empty() && bad_cats.is_empty() {
        return Ok(());
    }
    if declare_new {
        for a in &bad_accts {
            declare(&mut settings.accounts, a);
        }
        for c in &bad_cats {
            declare(&mut settings.categories, c);
        }
        return settings.save(palace.root());
    }
    let mut lines = vec!["ledger: undeclared name(s) — declare them or pass -n to add now:".to_string()];
    lines.extend(bad_accts.iter().map(|a| format!("  @{a}  (plc ledger declare {a} --physical)")));
    lines.extend(bad_cats.iter().map(|c| format!("  #{c}  (plc ledger declare {c} --ephemeral)")));
    Err(lines.join("\n"))
}

/// Insert `name` into a sorted, de-duplicated declared list.
fn declare(list: &mut Vec<String>, name: &str) {
    if let Err(pos) = list.binary_search(&name.to_string()) {
        list.insert(pos, name.to_string());
    }
}

/// The kind selected by `--physical`/`--ephemeral`, or `None` when neither is
/// given (bare list / import-both).
fn declare_kind(args: &DeclareArgs) -> Option<Decl> {
    match (args.physical, args.ephemeral) {
        (true, _) => Some(Decl::Account),
        (_, true) => Some(Decl::Category),
        _ => None,
    }
}

/// `plc ledger declare`: one command for both accounts (`--physical`) and
/// categories (`--ephemeral`). Bare lists everything; NAME(s) add (or remove
/// with `-r`); `--import` seeds from used names.
fn declare_cmd(palace: &Palace, args: DeclareArgs) -> Result<String, String> {
    let mut settings = Settings::load(palace.root());
    let kind = declare_kind(&args);

    if !args.names.is_empty() {
        let kind = kind.ok_or_else(|| {
            "ledger declare: say which kind — --physical (account) or --ephemeral (category)".to_string()
        })?;
        let names: Vec<String> = args
            .names
            .iter()
            .map(|n| clean_link(kind.label(), n).map(|n| ledger::normalize_name(&n)))
            .collect::<Result<_, _>>()?;
        // A name can't be both an account and a category.
        if !args.rm {
            let other = match kind {
                Decl::Account => &settings.categories,
                Decl::Category => &settings.accounts,
            };
            let clash: Vec<&String> = names.iter().filter(|n| other.contains(n)).collect();
            if !clash.is_empty() {
                let other_kind = match kind {
                    Decl::Account => "a category",
                    Decl::Category => "an account",
                };
                return Err(format!(
                    "ledger declare: already declared as {other_kind}: {} — a name can't be both @ and #",
                    clash.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }
        }
        let list = pick(&mut settings, kind);
        if args.rm {
            list.retain(|n| !names.contains(n));
        } else {
            names.iter().for_each(|n| declare(list, n));
        }
        settings.save(palace.root())?;
        let verb = if args.rm { "removed" } else { "declared" };
        return Ok(format!("{verb} {} {}: {}", names.len(), kind.plural(), names.join(", ")));
    }

    if args.import {
        let root = palace.root().join("notes/management/daily");
        let (used_accts, used_cats) = ledger::names(&root, &currency_from(&settings))?;
        let mut done = Vec::new();
        for k in kind.map_or(vec![Decl::Account, Decl::Category], |k| vec![k]) {
            let used = match k {
                Decl::Account => &used_accts,
                Decl::Category => &used_cats,
            };
            let before = pick(&mut settings, k).len();
            used.iter().for_each(|n| declare(pick(&mut settings, k), n));
            done.push(format!("{} {}", pick(&mut settings, k).len() - before, k.plural()));
        }
        settings.save(palace.root())?;
        return Ok(format!("imported {} from ledgers", done.join(", ")));
    }

    // Bare (or kind-filtered): list.
    Ok(match kind {
        Some(k) => list_kind(&settings, k),
        None => format!("{}\n{}", list_kind(&settings, Decl::Account), list_kind(&settings, Decl::Category)),
    })
}

/// Render one declared set as a titled block (or an empty-state hint).
fn list_kind(settings: &Settings, kind: Decl) -> String {
    let list = match kind {
        Decl::Account => &settings.accounts,
        Decl::Category => &settings.categories,
    };
    let (plural, sigil, flag) = (kind.plural(), kind.sigil(), kind.flag());
    if list.is_empty() {
        return format!("{plural}: (none — add with `plc ledger declare NAME {flag}`)");
    }
    let names: Vec<String> = list.iter().map(|n| format!("  {sigil}{n}")).collect();
    format!("{plural}:\n{}", names.join("\n"))
}

/// Split `used` names against a `declared` set: `(undeclared, unused)` —
/// names used but never declared, and names declared but never used.
fn diff_names(used: &[String], declared: &[String]) -> (Vec<String>, Vec<String>) {
    let undeclared = used.iter().filter(|n| !declared.contains(n)).cloned().collect();
    let unused = declared.iter().filter(|n| !used.contains(n)).cloned().collect();
    (undeclared, unused)
}

/// The ledger health section of `plc doctor`: compare `.plc/config` against the
/// names actually used in the ledgers, report anything off, and propose (or,
/// with `--fix`, apply) repairs.
pub fn doctor(palace: &Palace, fix: bool) -> Result<String, String> {
    let mut settings = Settings::load(palace.root());
    let root = palace.root().join("notes/management/daily");
    let (used_accts, used_cats) = ledger::names(&root, &currency_from(&settings))?;

    let mut findings: Vec<String> = Vec::new();
    let mut fixable = 0usize; // problems `--fix` can repair
    let mut fixed: Vec<String> = Vec::new();

    // A name declared as both an account and a category (can't auto-fix — which
    // side is right is your call).
    let both: Vec<String> = settings.accounts.iter().filter(|a| settings.categories.contains(*a)).cloned().collect();
    if !both.is_empty() {
        findings.push(format!("  ! {} name(s) declared as both @ and #:", both.len()));
        findings.extend(both.iter().map(|n| {
            format!("      {n}  (drop one: plc ledger declare {n} --physical -r | --ephemeral -r)")
        }));
    }

    for (kind, used) in [(Decl::Account, &used_accts), (Decl::Category, &used_cats)] {
        let declared = pick(&mut settings, kind).clone();
        let (sigil, flag, plural) = (kind.sigil(), kind.flag(), kind.plural());
        if declared.is_empty() {
            if !used.is_empty() {
                findings.push(format!(
                    "  · {plural}: guard off ({} used, none declared) — `plc ledger declare --import {flag}` to enable",
                    used.len()
                ));
            }
            continue;
        }
        let (undeclared, unused) = diff_names(used, &declared);
        if !undeclared.is_empty() {
            fixable += undeclared.len();
            findings.push(format!("  ! {} {plural} used but not declared:", undeclared.len()));
            findings.extend(undeclared.iter().map(|n| format!("      {sigil}{n}  (plc ledger declare {n} {flag})")));
            if fix {
                undeclared.iter().for_each(|n| declare(pick(&mut settings, kind), n));
                fixed.push(format!("declared {} {plural}", undeclared.len()));
            }
        }
        if !unused.is_empty() {
            findings.push(format!("  ! {} {plural} declared but never used (typo/stale?):", unused.len()));
            findings.extend(unused.iter().map(|n| format!("      {sigil}{n}  (plc ledger declare {n} {flag} -r)")));
        }
    }

    // No default currency pinned in the config (relying on PLC_CURRENCY/EUR).
    if settings.currency.is_none() {
        let dominant = ledger::currencies(&root, &currency_from(&settings))?
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(c, _)| c);
        match dominant {
            Some(c) => {
                fixable += 1;
                findings.push(format!("  ! no default currency in .plc/config — ledgers use {c}"));
                findings.push(format!("      set it: add `currency = {c}` (or `plc doctor --fix`)"));
                if fix {
                    settings.currency = Some(c.clone());
                    fixed.push(format!("set currency = {c}"));
                }
            }
            None => findings.push("  · no default currency in .plc/config (defaults to EUR)".to_string()),
        }
    }

    // Transactions with no stable `^id` (pre-id ledgers). `--fix` backfills a
    // frozen content-hash id onto each; the write happens inside `backfill_ids`.
    let cur = currency_from(&settings);
    let missing = ledger::backfill_ids(&root, &cur, fix)?;
    if missing > 0 {
        fixable += missing;
        findings.push(format!("  ! {missing} transaction(s) missing a stable id"));
        findings.push("      assign them: plc doctor --fix (frozen git-style ^id)".to_string());
        if fix {
            fixed.push(format!("assigned {missing} transaction id(s)"));
        }
    }

    // Two transactions sharing an id — a handle should be unique (not auto-fixable;
    // edit one so it re-seeds). Runs after any backfill above, so it also catches
    // ids just assigned to genuinely identical entries.
    let dups = ledger::duplicate_ids(&root, &cur)?;
    if !dups.is_empty() {
        findings.push(format!("  ! {} duplicate transaction id(s) — edit one so it re-seeds:", dups.len()));
        findings.extend(dups.iter().map(|id| format!("      ^{id}")));
    }

    // A pre-`.plc` do-pointer left at the vault root.
    let legacy = palace.root().join(".last-do");
    if legacy.is_file() {
        fixable += 1;
        findings.push("  ! legacy do-pointer at <root>/.last-do — should live in .plc/".to_string());
        if fix {
            std::fs::create_dir_all(palace.state_dir()).map_err(|e| format!("ledger doctor: {e}"))?;
            std::fs::rename(&legacy, palace.state_dir().join("last-do")).map_err(|e| format!("ledger doctor: {e}"))?;
            fixed.push("migrated .last-do → .plc/last-do".to_string());
        }
    }

    if findings.is_empty() {
        return Ok("\n  Doctor — all good  OK".to_string());
    }
    let mut out = vec![String::new(), "  Doctor — .plc/config vs the ledgers".to_string(), String::new()];
    out.extend(findings);
    out.push(String::new());
    if fix {
        if !fixed.is_empty() {
            settings.save(palace.root())?;
            out.push(format!("  fixed: {}", fixed.join("; ")));
        } else {
            out.push("  nothing auto-fixable; the items above need a manual call".to_string());
        }
    } else if fixable > 0 {
        out.push("  run `plc doctor --fix` to apply the safe repairs (import undeclared, set currency, migrate pointer)".to_string());
    }
    Ok(out.join("\n"))
}

/// The declared list for a kind (mutable borrow of the right settings field).
fn pick(settings: &mut Settings, kind: Decl) -> &mut Vec<String> {
    match kind {
        Decl::Account => &mut settings.accounts,
        Decl::Category => &mut settings.categories,
    }
}

/// Build a [`Transaction`] from `add` args. `now` is the default timestamp used
/// when `--date` is absent (injected so tests are deterministic).
/// `declared_accounts` disambiguates a bare `-T` target (account vs category).
fn build_txn(
    args: AddArgs,
    now: DateTime<FixedOffset>,
    default_currency: &str,
    declared_accounts: &[String],
) -> Result<Transaction, String> {
    let amount = ledger::eval_amount(&args.amount)
        .ok_or_else(|| format!("ledger: invalid amount: {}", args.amount))?;
    let currency = args
        .currency
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| default_currency.to_string());

    // The core shape (account / kind / category-or-dest / assertion) comes either
    // from the symbolic `-T` spec or from the individual flags.
    let mut split = Vec::new();
    let (account, kind, other, assert) = if let Some(spec) = &args.txn {
        let s = parse_txn_spec(spec, declared_accounts)?;
        (s.account, s.kind, s.other, s.assert)
    } else {
        let account = args
            .account
            .as_deref()
            .ok_or("ledger add: need an account — pass -a ACCOUNT or -T SPEC")?;
        let account = clean_link("account", account)?;
        let (kind, other) = match (args.to, args.category, args.income) {
            (Some(dest), _, _) => (Kind::Transfer, Some(clean_link("account", &dest)?)),
            (None, cat, income) => {
                let kind = if income { Kind::Income } else { Kind::Expense };
                (kind, cat.map(|c| clean_link("category", &c)).transpose()?)
            }
        };
        // Split legs (if any) distribute `amount` across categories; they must
        // sum to it, and the single `other` category is dropped.
        split = parse_splits(&args.split)?;
        if !split.is_empty() {
            let sum: i64 = split.iter().map(|(_, a)| a).sum();
            if sum != amount {
                return Err(format!(
                    "ledger: split legs sum to {sum} minor units, not the stated total {amount}"
                ));
            }
        }
        let assert = match args.assert.as_deref() {
            Some(s) => Some(parse_balance(s)?),
            None => None,
        };
        let other = if split.is_empty() { other } else { None };
        (account, kind, other, assert)
    };

    // A transaction must move between *different* buckets. The account can't also
    // be its counterpart — not as a transfer destination, and not as a category
    // (a name may not be both `@` and `#` within one transaction).
    let clashes =
        other.as_deref() == Some(account.as_str()) || split.iter().any(|(c, _)| c == &account);
    if clashes {
        return Err(if matches!(kind, Kind::Transfer) {
            format!("ledger: source and destination are the same account (@{account})")
        } else {
            format!("ledger: @{account} and #{account} are the same name — use distinct names")
        });
    }

    let projects = args
        .project
        .iter()
        .map(|p| clean_link("project", p).map(|p| ledger::normalize_name(&p)))
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

    let mut txn = Transaction {
        id: None,
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
    };
    // Seed the frozen content-hash id now, so every added transaction is written
    // with a stable `^id` handle. `now` is injected, so this is deterministic.
    txn.id = Some(ledger::txn_id(&txn));
    Ok(txn)
}

/// The core of a transaction parsed from a `-T` spec.
struct TxnShape {
    account: String,
    kind: Kind,
    other: Option<String>,
    assert: Option<i64>,
}

/// Parse a symbolic `-T` spec into a transaction shape. The arrow shows the flow
/// of money and either side may be the account — the kind is derived from which
/// side is an account vs a category:
///   `src -> dst` / `dst <- src`  money flows src → dst
///     account → category = expense · category → account = income
///     account → account   = transfer
///   `A = N`  assert A's balance is N (no flow)
/// A name is an account when written `@name` (or bare and declared), a category
/// when `#name` (or bare and not declared).
fn parse_txn_spec(spec: &str, accounts: &[String]) -> Result<TxnShape, String> {
    if let Some((l, r)) = spec.split_once('=') {
        let assert = parse_balance(r.trim())?;
        return Ok(TxnShape { account: spec_account(l)?, kind: Kind::Expense, other: None, assert: Some(assert) });
    }
    // Reduce both arrows to a directed (src → dst) flow.
    let (src, dst) = if let Some((l, r)) = spec.split_once("->") {
        (l, r)
    } else if let Some((l, r)) = spec.split_once("<-") {
        (r, l)
    } else {
        return Err(format!("ledger add: -T wants `A -> B`, `B <- A`, or `A = N`; got: {spec}"));
    };
    let (src_acct, src) = classify_target(src, accounts)?;
    let (dst_acct, dst) = classify_target(dst, accounts)?;
    match (src_acct, dst_acct) {
        (true, true) => Ok(TxnShape { account: src, kind: Kind::Transfer, other: Some(dst), assert: None }),
        (true, false) => Ok(TxnShape { account: src, kind: Kind::Expense, other: Some(dst), assert: None }),
        (false, true) => Ok(TxnShape { account: dst, kind: Kind::Income, other: Some(src), assert: None }),
        (false, false) => Err(format!("ledger add: -T needs an account (declared, or `@name`) in `{spec}`")),
    }
}

/// The left-hand account of a `-T` spec: drop an optional `@`, validate, normalize.
fn spec_account(s: &str) -> Result<String, String> {
    let t = s.trim();
    let t = t.strip_prefix('@').unwrap_or(t).trim();
    Ok(ledger::normalize_name(&clean_link("account", t)?))
}

/// Classify a `-T` target as `(is_account, normalized_name)`: `@name` forces an
/// account, `#name` a category, and a bare name is an account iff it is declared.
fn classify_target(s: &str, accounts: &[String]) -> Result<(bool, String), String> {
    let t = s.trim();
    if let Some(x) = t.strip_prefix('@') {
        return Ok((true, ledger::normalize_name(&clean_link("account", x)?)));
    }
    if let Some(x) = t.strip_prefix('#') {
        return Ok((false, ledger::normalize_name(&clean_link("category", x)?)));
    }
    let name = ledger::normalize_name(&clean_link("name", t)?);
    let is_acct = accounts.contains(&name);
    Ok((is_acct, name))
}

/// Parse `--split CAT=AMOUNT` args into `(normalized category, minor units)`.
fn parse_splits(args: &[String]) -> Result<Vec<(String, i64)>, String> {
    args.iter()
        .map(|s| {
            let (cat, amt) = s
                .split_once('=')
                .ok_or_else(|| format!("ledger: --split wants CAT=AMOUNT, got: {s}"))?;
            let cat = ledger::normalize_name(&clean_link("split category", cat)?);
            let amount = ledger::eval_amount(amt)
                .ok_or_else(|| format!("ledger: invalid split amount: {amt}"))?;
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
    let minor = ledger::amount_to_minor(mag)
        .ok_or_else(|| format!("ledger: invalid assert balance: {s}"))?;
    Ok(if neg { -minor } else { minor })
}

/// Parse a `--date` value: a full `YYYY-MM-DD HH:MM:SS ±ZZZZ` timestamp, or a
/// bare `YYYY-MM-DD` taken as that day at local midnight.
fn parse_when(s: &str) -> Result<DateTime<FixedOffset>, String> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_str(s, ledger::TIMESTAMP_FMT) {
        return Ok(dt);
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        if let Some(naive) = d.and_hms_opt(0, 0, 0) {
            if let LocalResult::Single(dt) = Local.from_local_datetime(&naive) {
                return Ok(dt.fixed_offset());
            }
        }
    }
    Err(format!("ledger: invalid date (want YYYY-MM-DD or full timestamp): {s}"))
}

/// Validate a value destined for a `[[wikilink]]`: non-blank and free of the
/// brackets/newline that would break the link (mirrors the global `--tag` check).
fn clean_link(label: &str, raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("ledger: {label} must not be blank"));
    }
    if trimmed.contains(['[', ']', '\n']) {
        return Err(format!("ledger: {label} must not contain brackets or newlines: {trimmed}"));
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
            account: Some("cash".into()),
            txn: None,
            category: Some("coffee".into()),
            split: vec![],
            to: None,
            income: false,
            currency: Some("EUR".into()),
            project: vec![],
            date: None,
            cleared: false,
            pending: false,
            new: false,
            assert: None,
        }
    }

    /// A fixed "now" so `build_txn`'s default stamp is deterministic in tests.
    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_str("2026-07-19 11:28:22 +0200", ledger::TIMESTAMP_FMT).unwrap()
    }

    #[test]
    fn txn_spec_shapes() {
        let accts = vec!["revolut".to_string(), "cash".to_string()];
        let build = |spec: &str| {
            let mut a = add_args();
            (a.account, a.category, a.txn) = (None, None, Some(spec.to_string()));
            build_txn(a, now(), "EUR", &accts).unwrap()
        };
        // income: revolut <- salary
        let t = build("revolut <- salary");
        assert_eq!((t.kind, t.account.as_str(), t.other.as_deref()), (Kind::Income, "revolut", Some("salary")));
        // expense: target is an undeclared category
        let t = build("revolut -> food/out");
        assert_eq!((t.kind, t.account.as_str(), t.other.as_deref()), (Kind::Expense, "revolut", Some("food/out")));
        // transfer: target is a declared account
        let t = build("revolut -> cash");
        assert_eq!((t.kind, t.account.as_str(), t.other.as_deref()), (Kind::Transfer, "revolut", Some("cash")));
        // `@` forces a transfer even when undeclared
        let t = build("revolut -> @wallet");
        assert_eq!(t.kind, Kind::Transfer);
        assert_eq!(t.other.as_deref(), Some("wallet"));
        // associative: the account may sit on either side of the arrow
        let t = build("taxi <- revolut"); // category <- account == revolut -> taxi
        assert_eq!((t.kind, t.account.as_str(), t.other.as_deref()), (Kind::Expense, "revolut", Some("taxi")));
        let t = build("salary -> revolut"); // category -> account == revolut <- salary
        assert_eq!((t.kind, t.account.as_str(), t.other.as_deref()), (Kind::Income, "revolut", Some("salary")));
        // assertion: revolut = 2300
        let t = build("revolut = 2300");
        assert_eq!((t.account.as_str(), t.assert), ("revolut", Some(230000)));
        // two categories → no account → error
        let mut a = add_args();
        (a.account, a.category, a.txn) = (None, None, Some("taxi <- food".to_string()));
        assert!(build_txn(a, now(), "EUR", &accts).is_err());
    }

    #[test]
    fn rejects_transfer_to_same_account() {
        // via -T
        let mut a = add_args();
        (a.account, a.category, a.txn) = (None, None, Some("revolut -> revolut".to_string()));
        assert!(build_txn(a, now(), "EUR", &["revolut".to_string()]).is_err());
        // via flags
        let mut a = add_args();
        (a.account, a.category, a.to) = (Some("cash".into()), None, Some("cash".into()));
        assert!(build_txn(a, now(), "EUR", &[]).is_err());
    }

    #[test]
    fn rejects_account_and_category_same_name() {
        // expense with @revolut and #revolut
        let mut a = add_args();
        (a.account, a.category) = (Some("revolut".into()), Some("revolut".into()));
        assert!(build_txn(a, now(), "EUR", &[]).is_err());
        // forced via -T (#revolut category on an @revolut account)
        let mut a = add_args();
        (a.account, a.category, a.txn) = (None, None, Some("revolut -> #revolut".to_string()));
        assert!(build_txn(a, now(), "EUR", &["revolut".to_string()]).is_err());
        // a split leg sharing the account name
        let mut a = add_args();
        (a.account, a.category, a.amount, a.split) =
            (Some("cash".into()), None, "10".into(), vec!["cash=10".into()]);
        assert!(build_txn(a, now(), "EUR", &[]).is_err());
    }

    #[test]
    fn used_names_by_kind() {
        // Expense: account + category.
        let t = build_txn(add_args(), now(), "EUR", &[]).unwrap();
        assert_eq!(used_names(&t), (vec!["cash".into()], vec!["coffee".into()]));

        // Transfer: two accounts, no category.
        let mut a = add_args();
        (a.to, a.category) = (Some("savings".into()), None);
        let t = build_txn(a, now(), "EUR", &[]).unwrap();
        assert_eq!(used_names(&t), (vec!["cash".into(), "savings".into()], vec![]));

        // Split: account + each leg category.
        let mut a = add_args();
        (a.amount, a.category, a.split) = ("90".into(), None, vec!["food=60".into(), "tax=30".into()]);
        let t = build_txn(a, now(), "EUR", &[]).unwrap();
        assert_eq!(used_names(&t), (vec!["cash".into()], vec!["food".into(), "tax".into()]));
    }

    #[test]
    fn render_log_formats_records() {
        let recs = vec![
            LogRecord { path: "a/b+ledger.md".into(), entry: "$ -4.50 EUR  @[[cash]] #[[coffee]]\n    coffee".into() },
            LogRecord { path: "a/c+ledger.md".into(), entry: "$ -11.00 EUR  @[[revolut]] #[[food/out]]".into() },
        ];
        assert_eq!(
            render_log(&recs),
            "==== a/b+ledger.md\n$ -4.50 EUR  @[[cash]] #[[coffee]]\n    coffee\n\
             ==== a/c+ledger.md\n$ -11.00 EUR  @[[revolut]] #[[food/out]]\n"
        );
    }

    #[test]
    fn remove_last_block_strips_one_occurrence() {
        let entry = "$ -11.00 EUR  @[[revolut]] #[[food/out]]\n    takos";
        // Trailing block.
        let content = "isg\n\n[[ledger]]\n\n\
                       $ -4.50 EUR  @[[cash]] #[[coffee]]\n    coffee\n\
                       $ -11.00 EUR  @[[revolut]] #[[food/out]]\n    takos\n";
        let kept = remove_last_block(content, entry).unwrap();
        assert!(kept.contains("coffee") && !kept.contains("takos"), "{kept}");
        assert!(kept.ends_with('\n'));
        // Mid-file block (a later, earlier-dated entry follows) is still removed.
        let mid = format!("head\n\n{entry}\n$ -1.00 EUR  @[[cash]] #[[tea]]\n    tea\n");
        let kept = remove_last_block(&mid, entry).unwrap();
        assert!(!kept.contains("takos") && kept.contains("tea"), "{kept}");
        // A stale entry is refused.
        assert_eq!(remove_last_block(content, "$ -99.00 EUR  @[[x]] #[[y]]"), None);
    }

    #[test]
    fn diff_names_splits_undeclared_and_unused() {
        let used = vec!["cash".to_string(), "revolut".to_string(), "cofee".to_string()];
        let declared = vec!["cash".to_string(), "revolut".to_string(), "rent".to_string()];
        let (undeclared, unused) = diff_names(&used, &declared);
        assert_eq!(undeclared, vec!["cofee".to_string()]); // used, not declared
        assert_eq!(unused, vec!["rent".to_string()]); // declared, not used
    }

    #[test]
    fn declare_inserts_sorted_and_dedups() {
        let mut list = vec![];
        declare(&mut list, "rent");
        declare(&mut list, "food");
        declare(&mut list, "rent"); // dup ignored
        assert_eq!(list, vec!["food".to_string(), "rent".to_string()]);
    }

    #[test]
    fn maps_expense_args() {
        let t = build_txn(add_args(), now(), "EUR", &[]).unwrap();
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
        let t = build_txn(a, now(), "EUR", &[]).unwrap();
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
        let t = build_txn(a, now(), "EUR", &[]).unwrap();
        assert_eq!(t.kind, Kind::Transfer);
        assert_eq!((t.account.as_str(), t.other.as_deref()), ("cash", Some("checking")));
    }

    #[test]
    fn projects_normalized_slash_preserved() {
        let mut a = add_args();
        a.project = vec!["Japan-Trip/Work".into()];
        assert_eq!(build_txn(a, now(), "EUR", &[]).unwrap().projects, vec!["japan-trip/work"]);
    }

    #[test]
    fn date_flag_overrides_now_and_sets_state() {
        let mut a = add_args();
        a.date = Some("2026-07-15".into()); // date-only → that day at local midnight
        a.cleared = true;
        let t = build_txn(a, now(), "EUR", &[]).unwrap();
        assert_eq!(t.state, State::Cleared);
        assert_ne!(t.date, Some(now()));
        assert_eq!(t.date.unwrap().format("%Y-%m-%d").to_string(), "2026-07-15");
    }

    #[test]
    fn rejects_bracket_in_account() {
        let mut a = add_args();
        a.account = Some("ca[sh".into());
        assert!(build_txn(a, now(), "EUR", &[]).is_err());
    }

    #[test]
    fn split_legs_distribute_and_must_sum() {
        let mut a = add_args();
        a.amount = "90".into();
        a.category = None;
        a.memo = vec!["Costco".into()];
        a.split = vec!["food=60".into(), "household=25".into(), "tax=5".into()];
        let t = build_txn(a, now(), "EUR", &[]).unwrap();
        assert_eq!(t.amount, 9000);
        assert_eq!(t.other, None);
        assert_eq!(t.split, vec![("food".into(), 6000), ("household".into(), 2500), ("tax".into(), 500)]);

        // Legs that don't sum to the total are rejected.
        let mut bad = add_args();
        bad.amount = "90".into();
        bad.category = None;
        bad.split = vec!["food=60".into(), "tax=5".into()];
        assert!(build_txn(bad, now(), "EUR", &[]).is_err());
    }

    #[test]
    fn assert_flag_parses_signed_balance() {
        let mut a = add_args();
        a.assert = Some("-12.00".into());
        assert_eq!(build_txn(a, now(), "EUR", &[]).unwrap().assert, Some(-1200));
    }

    #[test]
    fn rejects_invalid_date() {
        let mut a = add_args();
        a.date = Some("18/07/2026".into());
        assert!(build_txn(a, now(), "EUR", &[]).is_err());
    }

    #[test]
    fn long_entry_wraps_when_added() {
        // Tags and memo drop to continuation lines (the head keeps the
        // accounting); every continuation line stays within the 79-col budget.
        let mut a = add_args();
        a.memo = vec!["latte".into()];
        a.project = vec!["japan-trip/leisure".into(), "work".into(), "reimbursable".into()];
        let entry = ledger::format_entry(&build_txn(a, now(), "EUR", &[]).unwrap());
        assert!(entry.contains('\n'), "should wrap: {entry}");
        for line in entry.lines().skip(1) {
            assert!(line.chars().count() <= 79, "continuation over 79: {line}");
        }
    }

    /// A bare `EditArgs` (locate mode); tests flip individual fields on.
    fn edit_args(id: &str) -> EditArgs {
        EditArgs {
            id: id.into(),
            amount: None,
            memo: None,
            account: None,
            category: None,
            to: None,
            income: false,
            expense: false,
            currency: None,
            project: vec![],
            no_projects: false,
            date: None,
            cleared: false,
            pending: false,
            uncleared: false,
            assert: None,
            no_assert: false,
        }
    }

    #[test]
    fn has_edits_distinguishes_locate_from_edit() {
        assert!(!has_edits(&edit_args("abc"))); // bare → locate
        let mut a = edit_args("abc");
        a.cleared = true;
        assert!(has_edits(&a));
    }

    #[test]
    fn edit_changes_fields_and_freezes_id() {
        let base = build_txn(add_args(), now(), "EUR", &[]).unwrap(); // expense @cash #coffee
        let id = base.id.clone();
        assert!(id.is_some(), "add stamps an id");
        let mut a = edit_args("x");
        a.amount = Some("12.50".into());
        a.memo = Some("team lunch".into());
        a.cleared = true;
        let t = apply_edits(base, &a).unwrap();
        assert_eq!((t.amount, t.memo.as_str(), t.state), (1250, "team lunch", State::Cleared));
        assert_eq!(t.kind, Kind::Expense); // unchanged
        assert_eq!(t.id, id); // the frozen handle survives the edit
    }

    #[test]
    fn edit_switches_direction() {
        let base = build_txn(add_args(), now(), "EUR", &[]).unwrap();
        let mut a = edit_args("x");
        a.to = Some("savings".into());
        let t = apply_edits(base.clone(), &a).unwrap();
        assert_eq!((t.kind, t.other.as_deref()), (Kind::Transfer, Some("savings")));

        let mut a = edit_args("x");
        a.income = true;
        assert_eq!(apply_edits(base, &a).unwrap().kind, Kind::Income);
    }

    #[test]
    fn edit_rejects_split_amount_and_same_name() {
        // Changing a split's total would unbalance the legs.
        let mut aa = add_args();
        (aa.amount, aa.category, aa.split) = ("90".into(), None, vec!["food=60".into(), "tax=30".into()]);
        let split = build_txn(aa, now(), "EUR", &[]).unwrap();
        let mut a = edit_args("x");
        a.amount = Some("100".into());
        assert!(apply_edits(split, &a).is_err());

        // @cash and #cash are the same name.
        let base = build_txn(add_args(), now(), "EUR", &[]).unwrap();
        let mut a = edit_args("x");
        a.category = Some("cash".into());
        assert!(apply_edits(base, &a).is_err());
    }
}
