//! Plain-text transaction parsing and formatting — the finance data contract.
//!
//! A ledger is an ordinary palace note (header + `[[ledger]]` tag) whose body
//! carries transaction lines. A transaction line has the shape:
//!
//! ```text
//! $ <±amount> [CUR]  @[[account]]  (#[[category]] | > @[[account2]])  [memo…]
//! ```
//!
//!   * A leading `$` (then whitespace) marks the line as a transaction; any
//!     other line is prose and parses to `None`.
//!   * `±amount` is a decimal in the major unit. `-` is an outflow (expense),
//!     `+`/none an inflow (income); for a transfer it is the magnitude moved.
//!     It is stored as an `i64` count of minor units (cents) to avoid float drift.
//!   * `CUR` is an optional ISO-ish code (2–5 uppercase letters); when omitted the
//!     caller's default currency is used. Reports subtotal per currency — there is
//!     no FX conversion in a text vault.
//!   * `@[[account]]` names the account and `#[[category]]` the category (role
//!     sigils, so order does not matter). Both stay `[[wikilinks]]`, so the
//!     existing link/orphans engine still sees `account`/`category` as targets.
//!   * `> @[[account2]]` marks a transfer: the amount moves from `account` to
//!     `account2` and there is no category.
//!   * Anything after the structured part is a free-text payee/memo.
//!
//! Both directions live here so a round-trip (`parse_line(format_line(t)) == t`)
//! pins the grammar.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

use chrono::{DateTime, FixedOffset, NaiveDate};
use walkdir::WalkDir;

/// Timestamp format for a transaction's instant — identical to the note stamp
/// line (`isg 2026-07-19 11:28:22 +0200`), so money and prose share one clock.
pub const TIMESTAMP_FMT: &str = "%Y-%m-%d %H:%M:%S %z";

use crate::{ascii_lower, normalize_target};

/// Max width of any emitted ledger line. The vault is reflowed to this many
/// columns, so a longer entry is written as a multi-line block instead (see
/// [`format_entry`]) to keep every line intact under reflow.
const MAX_LINE: usize = 66;

/// Normalize a `~` tag / project: ASCII-lowercase and trim, but **preserve `/`**
/// so a nested tag like `japan-trip/work` stays whole. This differs from
/// [`normalize_target`], which strips a `/`-path down to its basename for the
/// notes/orphans link graph.
pub fn normalize_tag(s: &str) -> String {
    ascii_lower(s.trim())
}

/// The default currency when a line omits an explicit code and the caller has
/// no other preference (`PLC_CURRENCY`, falling back to `EUR`).
pub fn default_currency() -> String {
    env::var("PLC_CURRENCY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "EUR".to_string())
}

/// Parse a positive major-unit amount (e.g. `"4.50"`) into minor units (`450`).
/// The magnitude only — direction is decided by the caller (expense/income/
/// transfer). `None` if malformed. Used by `plc fin add` on its `AMOUNT` arg.
pub fn amount_to_minor(s: &str) -> Option<i64> {
    parse_amount(s.trim()).map(|(_neg, minor)| minor)
}

/// Reconciliation state: whether the transaction has cleared the real-world
/// account. `*` = cleared, `!` = pending; the default is uncleared (no marker).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Uncleared,
    Cleared,
    Pending,
}

/// What a transaction does to its account(s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Outflow from `account` into `category` (amount is negative on the line).
    Expense,
    /// Inflow to `account` from `category` (amount is positive on the line).
    Income,
    /// Movement of `amount` from `account` to the destination account.
    Transfer,
}

/// One parsed transaction. `amount` is a non-negative magnitude in minor units;
/// direction lives in `kind`. `other` holds the category (expense/income) or the
/// destination account (transfer); it is `None` for an uncategorized expense/income.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub amount: i64,
    pub currency: String,
    pub kind: Kind,
    pub account: String,
    pub other: Option<String>,
    /// The transaction's instant (`2026-07-19 11:28:22 +0200`); `None` means
    /// "inherit the ledger file's day". `plc fin add` stamps this with now().
    pub date: Option<DateTime<FixedOffset>>,
    /// Reconciliation state (`*`/`!`/none).
    pub state: State,
    /// Cross-cutting project/event tags (`~[[japan-trip/work]]`); a grouping
    /// attribute, not a money leg — excluded from the balance.
    pub projects: Vec<String>,
    pub memo: String,
}

/// Render a transaction as its canonical line (always two decimal places).
pub fn format_line(t: &Transaction) -> String {
    let amount = format_amount(t.amount);
    let signed = match t.kind {
        Kind::Expense => format!("-{amount}"),
        Kind::Income => format!("+{amount}"),
        Kind::Transfer => amount,
    };
    let mut line = String::from("$ ");
    if let Some(ts) = t.date {
        line.push_str(&ts.format(TIMESTAMP_FMT).to_string());
        line.push(' ');
    }
    match t.state {
        State::Cleared => line.push_str("* "),
        State::Pending => line.push_str("! "),
        State::Uncleared => {}
    }
    line.push_str(&format!("{signed} {}  @[[{}]]", t.currency, t.account));
    match (t.kind, &t.other) {
        (Kind::Transfer, Some(dest)) => line.push_str(&format!(" > @[[{dest}]]")),
        (_, Some(cat)) => line.push_str(&format!(" #[[{cat}]]")),
        (_, None) => {}
    }
    for p in &t.projects {
        line.push_str(&format!(" ~[[{p}]]"));
    }
    if !t.memo.is_empty() {
        line.push_str(&format!("  {}", t.memo));
    }
    line
}

/// Render a transaction for the ledger file: the compact [`format_line`] when it
/// fits in [`MAX_LINE`], otherwise a multi-line **block** — a `$` head line
/// (amount / account / category only) followed by indented continuation lines
/// (each ≤ `MAX_LINE`) carrying the overflow tags and then the wrapped memo.
/// Round-trips through [`parse_entries`].
pub fn format_entry(t: &Transaction) -> String {
    let one = format_line(t);
    if one.chars().count() <= MAX_LINE {
        return one;
    }
    // Head line without tags/memo (kept as compact as the data allows).
    let head = Transaction { projects: Vec::new(), memo: String::new(), ..t.clone() };
    let mut lines = vec![format_line(&head)];
    let tokens = t.projects.iter().map(|p| format!("~[[{p}]]"));
    wrap_into(&mut lines, tokens);
    wrap_into(&mut lines, t.memo.split_whitespace().map(str::to_string));
    lines.join("\n")
}

/// Pack `tokens` into indented continuation lines, each kept within
/// [`MAX_LINE`] where possible (a single oversize token still gets its own line).
fn wrap_into(lines: &mut Vec<String>, tokens: impl Iterator<Item = String>) {
    const INDENT: &str = "    ";
    let mut cur = String::new();
    for tok in tokens {
        let candidate = if cur.is_empty() {
            format!("{INDENT}{tok}")
        } else {
            format!("{cur} {tok}")
        };
        if candidate.chars().count() > MAX_LINE && !cur.is_empty() {
            lines.push(std::mem::replace(&mut cur, format!("{INDENT}{tok}")));
        } else {
            cur = candidate;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
}

/// Parse one line into a [`Transaction`], or `None` if it is not a well-formed
/// transaction line (prose, blank, missing `$`/amount/account, malformed amount).
/// `default_currency` is used when the line carries no explicit code.
pub fn parse_line(line: &str, default_currency: &str) -> Option<Transaction> {
    let mut rest = line.trim_start().strip_prefix('$')?;
    // The `$` must be a standalone sigil, not the first char of some `$word`.
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }

    // Optional leading timestamp (`YYYY-MM-DD HH:MM:SS ±ZZZZ`) and reconciliation
    // state (`*`/`!`), before the amount. A missing timestamp inherits the file's day.
    let mut date = None;
    if let Some((ts, after)) = take_timestamp(rest) {
        date = Some(ts);
        rest = after;
    }
    let mut state = State::Uncleared;
    if let Some((tok, after)) = next_token(rest) {
        match tok {
            "*" => (state, rest) = (State::Cleared, after),
            "!" => (state, rest) = (State::Pending, after),
            _ => {}
        }
    }

    let (amount_tok, rest) = next_token(rest)?;
    let (neg, amount) = parse_amount(amount_tok)?;

    // An optional currency code sits between the amount and the account.
    let (currency, rest) = match next_token(rest) {
        Some((tok, after)) if is_currency_code(tok) => (tok.to_string(), after),
        _ => (default_currency.to_string(), rest),
    };

    // The account is mandatory; a line without one is not a transaction.
    let (acct_raw, rest) = take_sigil_link(rest, '@')?;
    let account = normalize_target(acct_raw);

    let rest = rest.trim_start();
    let (kind, other, mut rest) = if let Some(after_gt) = rest.strip_prefix('>') {
        let (dest_raw, after) = take_sigil_link(after_gt, '@')?;
        (Kind::Transfer, Some(normalize_target(dest_raw)), after)
    } else if let Some((cat_raw, after)) = take_sigil_link(rest, '#') {
        let kind = if neg { Kind::Expense } else { Kind::Income };
        (kind, Some(normalize_target(cat_raw)), after)
    } else {
        let kind = if neg { Kind::Expense } else { Kind::Income };
        (kind, None, rest)
    };

    // Zero or more `~[[tag]]` project tags sit between the account section and
    // the memo. (On a block head line there are none — they arrive on the
    // continuation lines handled by `parse_entries`.)
    let mut projects = Vec::new();
    while let Some(after) = take_project(rest.trim_start(), &mut projects) {
        rest = after;
    }

    let memo = rest.trim().to_string();
    Some(Transaction { amount, currency, kind, account, other, date, state, projects, memo })
}

/// If `s` begins with a `~[[tag]]`, push its normalized tag onto `projects` and
/// return the remainder; else `None`.
fn take_project<'a>(s: &'a str, projects: &mut Vec<String>) -> Option<&'a str> {
    if !s.starts_with("~[[") {
        return None;
    }
    let (tag_raw, after) = take_sigil_link(s, '~')?;
    projects.push(normalize_tag(tag_raw));
    Some(after)
}

/// Parse every transaction in ledger `content`, joining each block-form entry
/// (a `$` head line plus following indented continuation lines) into one
/// transaction. Non-transaction lines (header, blanks, prose) are skipped.
pub fn parse_entries(content: &str, default_currency: &str) -> Vec<Transaction> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(mut txn) = parse_line(lines[i], default_currency) else {
            i += 1;
            continue;
        };
        i += 1;
        let mut memo_parts: Vec<String> = Vec::new();
        if !txn.memo.is_empty() {
            memo_parts.push(std::mem::take(&mut txn.memo));
        }
        // Absorb indented continuation lines: leading `~[[tag]]`s, then memo.
        while i < lines.len() && is_continuation(lines[i]) {
            let mut rest = lines[i].trim();
            while let Some(after) = take_project(rest.trim_start(), &mut txn.projects) {
                rest = after;
            }
            let m = rest.trim();
            if !m.is_empty() {
                memo_parts.push(m.to_string());
            }
            i += 1;
        }
        txn.memo = memo_parts.join(" ");
        out.push(txn);
    }
    out
}

/// A block continuation line: indented (leading space/tab) and non-blank.
fn is_continuation(line: &str) -> bool {
    line.starts_with([' ', '\t']) && !line.trim().is_empty()
}

/// If `s` begins with a full timestamp (`date time ±offset`, three
/// whitespace-separated tokens in [`TIMESTAMP_FMT`]), parse and consume it,
/// returning the instant and the remainder. Otherwise `None` (no consumption) —
/// so a normal `-4.50 EUR @[[…]]` head is left untouched.
fn take_timestamp(s: &str) -> Option<(DateTime<FixedOffset>, &str)> {
    let (d, r1) = next_token(s)?;
    let (t, r2) = next_token(r1)?;
    let (z, rest) = next_token(r2)?;
    let dt = DateTime::parse_from_str(&format!("{d} {t} {z}"), TIMESTAMP_FMT).ok()?;
    Some((dt, rest))
}

/// Split off the first whitespace-delimited token, returning it and the rest
/// (with the leading whitespace already trimmed). `None` when nothing is left.
fn next_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    match s.find(char::is_whitespace) {
        Some(i) => Some((&s[..i], &s[i..])),
        None => Some((s, "")),
    }
}

/// Whether `tok` looks like a currency code: 2–5 uppercase ASCII letters. This
/// never collides with the `@`/`#`/`>`/`[` that start the account section.
fn is_currency_code(tok: &str) -> bool {
    (2..=5).contains(&tok.len()) && tok.bytes().all(|b| b.is_ascii_uppercase())
}

/// Parse a decimal magnitude (with an optional leading `+`/`-`) into
/// `(is_negative, minor_units)`. Accepts 0–2 fractional digits. `None` if
/// malformed (empty, non-digit, or more than two decimals).
fn parse_amount(tok: &str) -> Option<(bool, i64)> {
    let (neg, digits) = match tok.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, tok.strip_prefix('+').unwrap_or(tok)),
    };
    let (int_part, frac_part) = match digits.split_once('.') {
        Some((i, f)) => (i, f),
        None => (digits, ""),
    };
    if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if frac_part.len() > 2 || !frac_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let int: i64 = int_part.parse().ok()?;
    let frac: i64 = match frac_part.len() {
        0 => 0,
        1 => frac_part.parse::<i64>().ok()? * 10,
        _ => frac_part.parse().ok()?,
    };
    Some((neg, int.checked_mul(100)?.checked_add(frac)?))
}

/// Render minor units as a `<whole>.<cc>` decimal (always two places).
fn format_amount(minor: i64) -> String {
    let minor = minor.abs();
    format!("{}.{:02}", minor / 100, minor % 100)
}

/// Consume a `<sigil>[[inner]]` token at the start of `s` (after trimming
/// leading whitespace). Returns the **raw** inner text and the remaining string,
/// or `None` if the sigil/brackets are not there. Callers normalize (`@`/`#`
/// via `normalize_target`; `~` via `normalize_tag`).
fn take_sigil_link(s: &str, sigil: char) -> Option<(&str, &str)> {
    let s = s.trim_start();
    let inner_and_rest = s.strip_prefix(sigil)?.strip_prefix("[[")?;
    let end = inner_and_rest.find("]]")?;
    Some((&inner_and_rest[..end], &inner_and_rest[end + 2..]))
}

/// Aggregated totals for one currency. `categories`/`accounts` map a name to a
/// signed minor-unit total.
///
/// This is a double-entry ledger: every transaction moves an amount from a
/// *source* bucket to a *destination* bucket (`-amount` / `+amount`), and
/// categories are just accounts. So `accounts` carry their natural balance
/// (assets positive, income sources negative) and, together with `categories`,
/// the whole book sums to zero — see [`Self::residual`].
#[derive(Default, Debug, PartialEq, Eq)]
pub struct CurrencyTotals {
    pub income: i64,
    pub expense: i64,
    pub count: usize,
    pub categories: BTreeMap<String, i64>,
    pub accounts: BTreeMap<String, i64>,
    /// Spend grouped by `~` project tag (expense `+`, income `-`). A reporting
    /// side-map only — **not** part of [`Self::residual`], so tagging never
    /// unbalances the book.
    pub projects: BTreeMap<String, i64>,
}

impl CurrencyTotals {
    /// Income minus expense (transfers net to zero here).
    pub fn net(&self) -> i64 {
        self.income - self.expense
    }

    /// The book's residual: the signed sum over every account *and* category.
    /// Double-entry guarantees this is `0`; a non-zero value means a line was
    /// malformed or dropped — the integrity check surfaced in the report.
    pub fn residual(&self) -> i64 {
        self.accounts.values().sum::<i64>() + self.categories.values().sum::<i64>()
    }
}

/// Suspense bucket for expense/income with no explicit category, so every
/// transaction still has a destination and the book balances.
const UNCATEGORIZED: &str = "uncategorized";

/// The category leg of an expense/income, falling back to the suspense bucket.
fn category_of(t: &Transaction) -> String {
    t.other.clone().unwrap_or_else(|| UNCATEGORIZED.to_string())
}

/// Fold transactions into per-currency totals. Pure — no I/O — so the aggregation
/// rules are unit-testable directly. Keyed by currency code (sorted).
pub fn summarize(txns: &[Transaction]) -> BTreeMap<String, CurrencyTotals> {
    let mut per: BTreeMap<String, CurrencyTotals> = BTreeMap::new();
    for t in txns {
        let cur = per.entry(t.currency.clone()).or_default();
        cur.count += 1;
        match t.kind {
            Kind::Expense => {
                cur.expense += t.amount;
                // Money leaves the account (source) and lands in the expense
                // category (destination): -amount / +amount → nets to zero.
                // A missing category still needs a destination, so it falls into
                // a suspense bucket rather than unbalancing the book.
                *cur.accounts.entry(t.account.clone()).or_default() -= t.amount;
                *cur.categories.entry(category_of(t)).or_default() += t.amount;
            }
            Kind::Income => {
                cur.income += t.amount;
                // Money leaves the income category (source) and lands in the
                // account (destination).
                *cur.accounts.entry(t.account.clone()).or_default() += t.amount;
                *cur.categories.entry(category_of(t)).or_default() -= t.amount;
            }
            Kind::Transfer => {
                *cur.accounts.entry(t.account.clone()).or_default() -= t.amount;
                if let Some(dest) = &t.other {
                    *cur.accounts.entry(dest.clone()).or_default() += t.amount;
                }
            }
        }
        // Attribute the spend to each tag (expense = cost `+`, income = `-`;
        // transfers move nothing in/out, so 0). Side-map only, off the book.
        let proj_delta = match t.kind {
            Kind::Expense => t.amount,
            Kind::Income => -t.amount,
            Kind::Transfer => 0,
        };
        for p in &t.projects {
            *cur.projects.entry(p.clone()).or_default() += proj_delta;
        }
    }
    per
}

/// Which transactions a report includes. Empty/`None` fields impose no limit.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Restrict to this reconciliation state.
    pub state: Option<State>,
    /// Keep a transaction if any pattern (lowercase substring) matches its
    /// account, category/dest, a tag, or its memo. Multiple patterns OR together.
    pub patterns: Vec<String>,
    /// Effective date on/after this day (inclusive).
    pub since: Option<NaiveDate>,
    /// Effective date on/before this day (inclusive).
    pub until: Option<NaiveDate>,
}

impl Filter {
    /// Whether `t` (with effective date `eff`) passes every active criterion.
    fn matches(&self, t: &Transaction, eff: Option<NaiveDate>) -> bool {
        if let Some(s) = self.state {
            if t.state != s {
                return false;
            }
        }
        if self.since.is_some() || self.until.is_some() {
            let Some(d) = eff else { return false };
            if self.since.is_some_and(|s| d < s) || self.until.is_some_and(|u| d > u) {
                return false;
            }
        }
        if !self.patterns.is_empty() {
            let hit = |p: &String| {
                t.account.contains(p.as_str())
                    || t.other.as_deref().is_some_and(|o| o.contains(p.as_str()))
                    || t.projects.iter().any(|pr| pr.contains(p.as_str()))
                    || t.memo.to_lowercase().contains(p.as_str())
            };
            if !self.patterns.iter().any(hit) {
                return false;
            }
        }
        true
    }
}

/// Walk `root` for `*+ledger.md` files, parse every transaction, keep those that
/// pass `filter`, and return the formatted per-currency report. Mirrors the
/// walk/aggregate shape of [`crate::orphans::report`]. `default_currency` fills
/// in for lines that omit an explicit code. A transaction's effective date is
/// its own timestamp, or — when it has none — the ledger file's day.
pub fn report(
    root: &Path,
    default_currency: &str,
    filter: &Filter,
) -> Result<String, String> {
    if !root.is_dir() {
        return Err(format!("fin: cannot read {}", root.display()));
    }
    let mut txns: Vec<Transaction> = Vec::new();
    let mut ledger_files = 0usize;
    for entry in WalkDir::new(root).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.ends_with("+ledger.md") {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        ledger_files += 1;
        let file_day = file_day(name);
        for t in parse_entries(&content, default_currency) {
            let eff = t.date.map(|d| d.date_naive()).or(file_day);
            if filter.matches(&t, eff) {
                txns.push(t);
            }
        }
    }
    Ok(render(&summarize(&txns), ledger_files))
}

/// The day encoded in a `YYYY-MM-DD+ledger.md` filename, if present.
fn file_day(name: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(name.get(..10)?, "%Y-%m-%d").ok()
}

/// Format a summary as a human-readable block, in the visual style of
/// `orphans::report` (leading blank line, indented, no trailing newline).
fn render(summary: &BTreeMap<String, CurrencyTotals>, ledger_files: usize) -> String {
    let total: usize = summary.values().map(|c| c.count).sum();
    let mut lines: Vec<String> = Vec::new();
    lines.push(String::new());
    lines.push(format!(
        "  Finance — {total} transaction(s) across {ledger_files} ledger file(s)"
    ));
    if summary.is_empty() {
        lines.push(String::new());
        lines.push("  (no transactions found)".to_string());
        return lines.join("\n");
    }
    for (cur, t) in summary {
        lines.push(String::new());
        lines.push(format!("  {cur}"));
        lines.push(format!("    income   : {}", format_amount(t.income)));
        lines.push(format!("    expenses : {}", format_amount(t.expense)));
        lines.push(format!("    net      : {}", format_signed(t.net())));
        let residual = t.residual();
        let book = if residual == 0 {
            "0.00  ✓".to_string()
        } else {
            format!("{}  ✗ UNBALANCED", format_signed(residual))
        };
        lines.push(format!("    book     : {book}"));
        push_section(&mut lines, "by account", &t.accounts);
        push_section(&mut lines, "by category", &t.categories);
        push_section(&mut lines, "by project", &t.projects);
    }
    lines.join("\n")
}

/// Append a titled, signed-total section (skipped when empty).
fn push_section(lines: &mut Vec<String>, title: &str, rows: &BTreeMap<String, i64>) {
    if rows.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("    {title}"));
    for (name, total) in rows {
        lines.push(format!("      {name:<18}{}", format_signed(*total)));
    }
}

/// Signed major-unit rendering: `+2400.00`, `-4.50`, `+0.00`.
fn format_signed(minor: i64) -> String {
    let sign = if minor < 0 { '-' } else { '+' };
    format!("{sign}{}", format_amount(minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EUR: &str = "EUR";

    fn txn(amount: i64, currency: &str, kind: Kind, account: &str, other: Option<&str>) -> Transaction {
        Transaction {
            amount,
            currency: currency.into(),
            kind,
            account: account.into(),
            other: other.map(Into::into),
            date: None,
            state: State::Uncleared,
            projects: Vec::new(),
            memo: String::new(),
        }
    }

    fn expense() -> Transaction {
        Transaction {
            amount: 450,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "cash".into(),
            other: Some("coffee".into()),
            date: None,
            state: State::Uncleared,
            projects: Vec::new(),
            memo: "Blue Bottle".into(),
        }
    }

    #[test]
    fn round_trip_expense() {
        let t = expense();
        assert_eq!(format_line(&t), "$ -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle");
        assert_eq!(parse_line(&format_line(&t), EUR).as_ref(), Some(&t));
    }

    #[test]
    fn round_trip_income() {
        let t = Transaction {
            amount: 240000,
            currency: "EUR".into(),
            kind: Kind::Income,
            account: "checking".into(),
            other: Some("salary".into()),
            date: None,
            state: State::Uncleared,
            projects: Vec::new(),
            memo: "July pay".into(),
        };
        assert_eq!(format_line(&t), "$ +2400.00 EUR  @[[checking]] #[[salary]]  July pay");
        assert_eq!(parse_line(&format_line(&t), EUR).as_ref(), Some(&t));
    }

    #[test]
    fn round_trip_transfer() {
        let t = Transaction {
            amount: 20000,
            currency: "EUR".into(),
            kind: Kind::Transfer,
            account: "checking".into(),
            other: Some("cash".into()),
            date: None,
            state: State::Uncleared,
            projects: Vec::new(),
            memo: "ATM".into(),
        };
        assert_eq!(format_line(&t), "$ 200.00 EUR  @[[checking]] > @[[cash]]  ATM");
        assert_eq!(parse_line(&format_line(&t), EUR).as_ref(), Some(&t));
    }

    #[test]
    fn round_trip_no_category_no_memo() {
        let t = Transaction {
            amount: 1000,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "cash".into(),
            other: None,
            date: None,
            state: State::Uncleared,
            projects: Vec::new(),
            memo: String::new(),
        };
        assert_eq!(format_line(&t), "$ -10.00 EUR  @[[cash]]");
        assert_eq!(parse_line(&format_line(&t), EUR).as_ref(), Some(&t));
    }

    #[test]
    fn amount_to_minor_parses_magnitude() {
        assert_eq!(amount_to_minor("4.50"), Some(450));
        assert_eq!(amount_to_minor(" 12 "), Some(1200));
        assert_eq!(amount_to_minor("2400.00"), Some(240000));
        assert_eq!(amount_to_minor("4.500"), None);
        assert_eq!(amount_to_minor("x"), None);
    }

    #[test]
    fn custom_currency_kept() {
        let t = parse_line("$ -12.00 USD  @[[card]] #[[software]]  JetBrains", EUR).unwrap();
        assert_eq!(t.currency, "USD");
        assert_eq!(t.amount, 1200);
        assert_eq!(t.kind, Kind::Expense);
    }

    #[test]
    fn default_currency_used_when_omitted() {
        let t = parse_line("$ -4.50 @[[cash]] #[[coffee]]", EUR).unwrap();
        assert_eq!(t.currency, "EUR");
        assert_eq!(t.other.as_deref(), Some("coffee"));
        assert_eq!(t.memo, "");
    }

    #[test]
    fn one_decimal_and_integer_amounts() {
        assert_eq!(parse_line("$ -4.5 @[[cash]]", EUR).unwrap().amount, 450);
        assert_eq!(parse_line("$ +7 @[[cash]]", EUR).unwrap().amount, 700);
    }

    #[test]
    fn positive_without_sign_is_income() {
        assert_eq!(parse_line("$ 7 @[[cash]]", EUR).unwrap().kind, Kind::Income);
    }

    #[test]
    fn account_and_category_are_normalized() {
        let t = parse_line("$ -4.50 @[[Cash|wallet]] #[[Coffee#beans]]", EUR).unwrap();
        assert_eq!(t.account, "cash");
        assert_eq!(t.other.as_deref(), Some("coffee"));
    }

    #[test]
    fn leading_whitespace_tolerated() {
        assert!(parse_line("    $ -4.50 @[[cash]]", EUR).is_some());
    }

    #[test]
    fn prose_line_rejected() {
        assert!(parse_line("woke up, wrote for an hour", EUR).is_none());
        assert!(parse_line("$5 is not a transaction", EUR).is_none()); // `$` not a sigil
        assert!(parse_line("", EUR).is_none());
    }

    #[test]
    fn missing_account_rejected() {
        assert!(parse_line("$ -4.50 EUR", EUR).is_none());
        assert!(parse_line("$ -4.50 #[[coffee]]", EUR).is_none());
    }

    #[test]
    fn malformed_amount_rejected() {
        assert!(parse_line("$ abc @[[cash]]", EUR).is_none());
        assert!(parse_line("$ 4.500 @[[cash]]", EUR).is_none()); // >2 decimals
        assert!(parse_line("$ . @[[cash]]", EUR).is_none());
    }

    #[test]
    fn round_trip_with_timestamp_and_state() {
        let t = Transaction {
            amount: 450,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "cash".into(),
            other: Some("coffee".into()),
            date: DateTime::parse_from_str("2026-07-18 09:30:00 +0200", TIMESTAMP_FMT).ok(),
            state: State::Cleared,
            projects: Vec::new(),
            memo: "Blue Bottle".into(),
        };
        assert_eq!(
            format_line(&t),
            "$ 2026-07-18 09:30:00 +0200 * -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle"
        );
        assert_eq!(parse_line(&format_line(&t), EUR).as_ref(), Some(&t));
    }

    #[test]
    fn pending_marker_round_trips() {
        let t = parse_line("$ ! -4.50 @[[cash]] #[[coffee]]", EUR).unwrap();
        assert_eq!(t.state, State::Pending);
        assert_eq!(t.date, None);
        assert_eq!(format_line(&t), "$ ! -4.50 EUR  @[[cash]] #[[coffee]]");
    }

    #[test]
    fn no_date_no_state_is_backward_compatible() {
        // Existing lines (no date/state) parse to None/Uncleared and reprint
        // byte-identically — nothing before the amount.
        let t = parse_line("$ -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle", EUR).unwrap();
        assert_eq!(t.date, None);
        assert_eq!(t.state, State::Uncleared);
        assert_eq!(format_line(&t), "$ -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle");
    }

    #[test]
    fn report_filters_by_state() {
        let dir = std::env::temp_dir().join(format!("plc-finstate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let sub = dir.join("2026/07");
        fs::create_dir_all(&sub).unwrap();
        fs::write(
            sub.join("2026-07-19+ledger.md"),
            "isg\n\n[[ledger]]\n\n$ * -4.50 EUR @[[cash]] #[[coffee]]\n$ -9.00 EUR @[[cash]] #[[lunch]]\n",
        )
        .unwrap();

        let all = report(&dir, EUR, &Filter::default()).unwrap();
        assert!(all.contains("coffee") && all.contains("lunch"), "{all}");
        let only = Filter { state: Some(State::Cleared), ..Filter::default() };
        let cleared = report(&dir, EUR, &only).unwrap();
        assert!(cleared.contains("coffee") && !cleared.contains("lunch"), "{cleared}");

        fs::remove_dir_all(&dir).ok();
    }

    /// Write one ledger file `name` with `body` under a temp dir; return the dir.
    fn ledger_dir(tag: &str, name: &str, body: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("plc-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let sub = dir.join("2026/07");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(name), body).unwrap();
        dir
    }

    #[test]
    fn report_filters_by_pattern() {
        let dir = ledger_dir(
            "finpat",
            "2026-07-19+ledger.md",
            "$ -4.50 EUR @[[cash]] #[[coffee]]\n$ -900 EUR @[[bnp]] #[[rent]]\n",
        );
        let f = Filter { patterns: vec!["coffee".into()], ..Filter::default() };
        let out = report(&dir, EUR, &f).unwrap();
        assert!(out.contains("coffee") && !out.contains("rent"), "{out}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn report_filters_by_date_range() {
        let dir = ledger_dir(
            "findate",
            "2026-07-19+ledger.md",
            "$ 2026-07-01 00:00:00 +0200 -900 EUR @[[bnp]] #[[rent]]\n\
             $ 2026-07-18 00:00:00 +0200 -4.50 EUR @[[cash]] #[[coffee]]\n",
        );
        let f = Filter { since: NaiveDate::from_ymd_opt(2026, 7, 10), ..Filter::default() };
        let out = report(&dir, EUR, &f).unwrap();
        assert!(out.contains("coffee") && !out.contains("rent"), "{out}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn effective_date_falls_back_to_file_day() {
        // No explicit timestamp → the entry's date is the file's day (2026-07-05).
        let dir = ledger_dir("finfday", "2026-07-05+ledger.md", "$ -4.50 EUR @[[cash]] #[[coffee]]\n");
        let after = Filter { since: NaiveDate::from_ymd_opt(2026, 7, 10), ..Filter::default() };
        assert!(!report(&dir, EUR, &after).unwrap().contains("coffee"));
        let upto = Filter { until: NaiveDate::from_ymd_opt(2026, 7, 6), ..Filter::default() };
        assert!(report(&dir, EUR, &upto).unwrap().contains("coffee"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn round_trip_with_projects() {
        let t = Transaction {
            amount: 8000,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "card".into(),
            other: Some("food".into()),
            date: None,
            state: State::Uncleared,
            projects: vec!["trip".into()],
            memo: "ramen".into(),
        };
        assert_eq!(format_line(&t), "$ -80.00 EUR  @[[card]] #[[food]] ~[[trip]]  ramen");
        // Short enough → one line; round-trips.
        assert_eq!(format_entry(&t), format_line(&t));
        assert_eq!(parse_entries(&format_entry(&t), EUR), vec![t]);
    }

    #[test]
    fn tags_preserve_slash_hierarchy() {
        let t = parse_line("$ -80 @[[card]] #[[food]] ~[[Japan-Trip/Work]]  x", EUR).unwrap();
        assert_eq!(t.projects, vec!["japan-trip/work"]); // lowercased, `/` kept
        assert_eq!(t.memo, "x");
    }

    #[test]
    fn long_entry_wraps_to_block_and_round_trips() {
        let t = Transaction {
            amount: 45,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "cash".into(),
            other: Some("coffee".into()),
            date: None,
            state: State::Uncleared,
            projects: vec!["japan-trip/leisure".into(), "work".into()],
            memo: "latte at the airport before the long flight home".into(),
        };
        let block = format_entry(&t);
        assert!(block.contains('\n'), "should wrap: {block}");
        for line in block.lines() {
            assert!(line.chars().count() <= 66, "line over 66: {line:?}");
        }
        assert_eq!(parse_entries(&block, EUR), vec![t]);
    }

    #[test]
    fn projects_accumulate_and_stay_off_the_book() {
        let mut a = txn(6000, "EUR", Kind::Expense, "card", Some("food"));
        a.projects = vec!["japan-trip".into()];
        let mut b = txn(30000, "EUR", Kind::Expense, "card", Some("hotel"));
        b.projects = vec!["japan-trip".into()];
        let s = summarize(&[a, b]);
        let eur = &s["EUR"];
        assert_eq!(eur.projects["japan-trip"], 36000); // 60 + 300, spend positive
        assert_eq!(eur.residual(), 0); // projects excluded → book still balances
    }

    #[test]
    fn summarize_net_categories_and_balances() {
        let txns = vec![
            txn(450, "EUR", Kind::Expense, "cash", Some("coffee")),
            txn(240000, "EUR", Kind::Income, "checking", Some("salary")),
            txn(20000, "EUR", Kind::Transfer, "checking", Some("cash")),
        ];
        let s = summarize(&txns);
        let eur = &s["EUR"];
        assert_eq!(eur.income, 240000);
        assert_eq!(eur.expense, 450);
        assert_eq!(eur.net(), 239550);
        // Transfer moves value both ways: cash gains 20000 (minus the 450 expense);
        // checking loses 20000 (on top of the 240000 income).
        assert_eq!(eur.accounts["cash"], 19550);
        assert_eq!(eur.accounts["checking"], 220000);
        // Double-entry signs: an expense flows *into* its category (positive);
        // an income flows *out of* its category (negative). Transfers touch none.
        assert_eq!(eur.categories["coffee"], 450);
        assert_eq!(eur.categories["salary"], -240000);
        assert!(!eur.categories.contains_key("cash"));
        // The whole book — accounts plus categories — sums to zero.
        assert_eq!(eur.residual(), 0);
    }

    #[test]
    fn book_always_balances() {
        // A mix of every kind, including an opening-balance income and a
        // wallet-funding transfer, still nets to zero across the book.
        let txns = vec![
            txn(320000, "EUR", Kind::Income, "bnp", Some("opening")),
            txn(240000, "EUR", Kind::Income, "bnp", Some("salary")),
            txn(20000, "EUR", Kind::Transfer, "bnp", Some("cash")),
            txn(450, "EUR", Kind::Expense, "cash", Some("coffee")),
            txn(1200, "EUR", Kind::Expense, "card", None), // uncategorized
        ];
        assert_eq!(summarize(&txns)["EUR"].residual(), 0);
    }

    #[test]
    fn summarize_groups_per_currency() {
        let txns = vec![
            txn(1200, "USD", Kind::Expense, "card", None),
            txn(500, "EUR", Kind::Expense, "cash", None),
        ];
        let s = summarize(&txns);
        assert_eq!(s.len(), 2);
        assert_eq!(s["USD"].expense, 1200);
        assert_eq!(s["EUR"].expense, 500);
    }

    #[test]
    fn report_walks_only_ledger_files() {
        let dir = std::env::temp_dir().join(format!("plc-finrep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let sub = dir.join("2026/07");
        fs::create_dir_all(&sub).unwrap();
        fs::write(
            sub.join("2026-07-19+ledger.md"),
            "isg\n\n[[ledger]]\n\n$ -4.50 EUR  @[[cash]] #[[coffee]]  x\n$ +100 EUR @[[cash]] #[[gift]]\n",
        )
        .unwrap();
        // A normal daily note (no +ledger) must be ignored, even if it holds a `$` line.
        fs::write(sub.join("2026-07-19.md"), "$ -999 EUR @[[cash]]\n").unwrap();

        let out = report(&dir, EUR, &Filter::default()).unwrap();
        assert!(out.contains("EUR"), "{out}");
        assert!(out.contains("net"), "{out}");
        assert!(out.contains("coffee"), "{out}");
        assert!(!out.contains("999"), "non-ledger file leaked: {out}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn report_empty_when_no_ledgers() {
        let dir = std::env::temp_dir().join(format!("plc-finempty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let out = report(&dir, EUR, &Filter::default()).unwrap();
        assert!(out.contains("no transactions found"), "{out}");
        fs::remove_dir_all(&dir).ok();
    }
}
