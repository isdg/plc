//! Plain-text transaction parsing and formatting — the ledger data contract.
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

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::calendar::last_day_of_month;

/// Timestamp format for a transaction's instant — identical to the note stamp
/// line (`isg 2026-07-19 11:28:22 +0200`), so money and prose share one clock.
pub const TIMESTAMP_FMT: &str = "%Y-%m-%d %H:%M:%S %z";

use crate::ascii_lower;

/// Max width of any emitted continuation (memo/tag) line. The vault is reflowed
/// to this many columns, so long memos are wrapped to keep every line intact
/// under reflow. The `$` head line (the accounting) is never split — see
/// [`format_entry`].
const MAX_LINE: usize = 79;

/// Normalize a ledger name — an account (`@`), category (`#`), or tag (`~`):
/// drop any `|alias`/`#heading`/`^block` suffix, then ASCII-lowercase and trim,
/// but **preserve `/`** so a nested name like `bank/checking` or
/// `japan-trip/work` stays whole (for tree rollup). Differs from
/// `crate::normalize_target`, which additionally strips a `/`-path to its
/// basename for the notes/orphans link graph.
pub fn normalize_name(s: &str) -> String {
    let s = s.trim();
    let end = s.find(['|', '#', '^']).unwrap_or(s.len());
    ascii_lower(s[..end].trim())
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
/// transfer). `None` if malformed. Used by `plc ledger add` on its `AMOUNT` arg.
pub fn amount_to_minor(s: &str) -> Option<i64> {
    parse_amount(s.trim()).map(|(_neg, minor)| minor)
}

/// Evaluate an amount *expression* — `+ - * / ( )` over decimal numbers — to
/// minor units, rounded to the nearest cent (magnitude only, like
/// [`amount_to_minor`], which it supersedes for `plc ledger add`). So
/// `"3*4.50+1"` → `1450`. `None` on malformed input or division by zero.
pub fn eval_amount(s: &str) -> Option<i64> {
    let toks = tokenize(s)?;
    let mut ev = Eval { toks: &toks, i: 0 };
    let v = ev.expr()?;
    if ev.i != toks.len() || !v.is_finite() {
        return None; // trailing junk or NaN/inf
    }
    Some((v * 100.0).round().abs() as i64)
}

#[derive(PartialEq)]
enum Tok {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

/// Lex an amount expression; `None` on any unexpected character or empty input.
fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let mut toks = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' => {
                chars.next();
            }
            '+' => tok(&mut toks, &mut chars, Tok::Plus),
            '-' => tok(&mut toks, &mut chars, Tok::Minus),
            '*' => tok(&mut toks, &mut chars, Tok::Star),
            '/' => tok(&mut toks, &mut chars, Tok::Slash),
            '(' => tok(&mut toks, &mut chars, Tok::LParen),
            ')' => tok(&mut toks, &mut chars, Tok::RParen),
            '0'..='9' | '.' => {
                let mut num = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' {
                        num.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                toks.push(Tok::Num(num.parse().ok()?));
            }
            _ => return None,
        }
    }
    (!toks.is_empty()).then_some(toks)
}

fn tok(toks: &mut Vec<Tok>, chars: &mut std::iter::Peekable<std::str::Chars>, t: Tok) {
    toks.push(t);
    chars.next();
}

/// A recursive-descent evaluator: `expr → term (+|- term)*`,
/// `term → factor (*|/ factor)*`, `factor → (-|+)? primary`,
/// `primary → Num | ( expr )`.
struct Eval<'a> {
    toks: &'a [Tok],
    i: usize,
}

impl Eval<'_> {
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(t) = self.toks.get(self.i) {
            match t {
                Tok::Plus => {
                    self.i += 1;
                    v += self.term()?;
                }
                Tok::Minus => {
                    self.i += 1;
                    v -= self.term()?;
                }
                _ => break,
            }
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.factor()?;
        while let Some(t) = self.toks.get(self.i) {
            match t {
                Tok::Star => {
                    self.i += 1;
                    v *= self.factor()?;
                }
                Tok::Slash => {
                    self.i += 1;
                    let d = self.factor()?;
                    if d == 0.0 {
                        return None;
                    }
                    v /= d;
                }
                _ => break,
            }
        }
        Some(v)
    }

    fn factor(&mut self) -> Option<f64> {
        match self.toks.get(self.i)? {
            Tok::Minus => {
                self.i += 1;
                Some(-self.factor()?)
            }
            Tok::Plus => {
                self.i += 1;
                self.factor()
            }
            Tok::Num(n) => {
                let n = *n;
                self.i += 1;
                Some(n)
            }
            Tok::LParen => {
                self.i += 1;
                let v = self.expr()?;
                if self.toks.get(self.i) == Some(&Tok::RParen) {
                    self.i += 1;
                    Some(v)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
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
    /// A stable git-style short hash (12 hex chars), seeded from the
    /// transaction's content at creation and then **frozen** — an immutable
    /// handle for editing tooling, not a live checksum, so a later edit does
    /// not change it. `None` on a legacy line without one (until `plc doctor`
    /// backfills it). See [`txn_id`].
    pub id: Option<String>,
    pub amount: i64,
    pub currency: String,
    pub kind: Kind,
    pub account: String,
    pub other: Option<String>,
    /// Balance assertion: the asserted balance of `account` (this currency's
    /// minor units, signed) immediately after this transaction; `None` if unasserted.
    pub assert: Option<i64>,
    /// The transaction's instant (`2026-07-19 11:28:22 +0200`); `None` means
    /// "inherit the ledger file's day". `plc ledger add` stamps this with now().
    pub date: Option<DateTime<FixedOffset>>,
    /// Reconciliation state (`*`/`!`/none).
    pub state: State,
    /// Cross-cutting project/event tags (`~[[japan-trip/work]]`); a grouping
    /// attribute, not a money leg — excluded from the balance.
    pub projects: Vec<String>,
    /// Split legs: `(category, magnitude)` pairs distributing one payment across
    /// several categories. Empty for a simple transaction (which uses `other`);
    /// when non-empty, `other` is `None` and `amount` equals their sum.
    pub split: Vec<(String, i64)>,
    pub memo: String,
}

/// Render a transaction as its canonical line (always two decimal places).
pub fn format_line(t: &Transaction) -> String {
    let signed = signed_amount(t);
    let mut line = String::from("$ ");
    if let Some(id) = &t.id {
        line.push_str(&format!("^{id} "));
    }
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
    if let Some(a) = t.assert {
        let sign = if a < 0 { "-" } else { "" };
        line.push_str(&format!(" = {sign}{} {}", format_amount(a), t.currency));
    }
    for p in &t.projects {
        line.push_str(&format!(" ~[[{p}]]"));
    }
    if !t.memo.is_empty() {
        line.push_str(&format!("  {}", t.memo));
    }
    line
}

/// Render a transaction for the ledger file. The `$` head line carries the whole
/// accounting — timestamp, state, amount, account, category / transfer
/// destination, balance assertion, and `~` tags — always on one line, so
/// `@account` and `#category` never separate. The memo, when present, always
/// drops to its own indented continuation line(s), wrapped at [`MAX_LINE`].
/// Round-trips through [`parse_entries`].
pub fn format_entry(t: &Transaction) -> String {
    if !t.split.is_empty() {
        return format_split(t);
    }
    let head = Transaction { memo: String::new(), ..t.clone() };
    let mut lines = vec![format_line(&head)];
    wrap_into(&mut lines, t.memo.split_whitespace().map(str::to_string));
    lines.join("\n")
}

/// Seed a transaction's stable id: the first 12 hex chars of the SHA-256 of its
/// canonical [`format_entry`] block rendered **without** an id. Deterministic
/// and content-addressed (git-style), but assigned once and then frozen onto the
/// line — a later edit does not change the stored id. Because the canonical form
/// includes the timestamp, transactions a second apart get distinct ids.
pub fn txn_id(t: &Transaction) -> String {
    let bare = Transaction { id: None, ..t.clone() };
    let digest = Sha256::digest(format_entry(&bare).as_bytes());
    to_hex(&digest[..6]) // 6 bytes → 12 hex chars
}

/// Lowercase-hex encoding of `bytes` (avoids pulling in the `hex` crate).
fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Render a split transaction as a block: a head line carrying the (signed)
/// total on its `@account` — with the `~` tags trailing the account, as in a
/// simple entry — then one `#[[category]] ±amount` posting line per split leg
/// (signed like the total), then the wrapped memo.
fn format_split(t: &Transaction) -> String {
    // Head: the total on the account, tags trailing it; no category/split/memo.
    let head = Transaction {
        other: None,
        assert: None,
        split: Vec::new(),
        memo: String::new(),
        ..t.clone() // projects retained so `~` tags stay on the head line
    };
    let sign = if matches!(t.kind, Kind::Income) { "+" } else { "-" };
    let mut lines = vec![format_line(&head)];
    for (cat, amt) in &t.split {
        lines.push(format!("    #[[{cat}]]  {sign}{} {}", format_amount(*amt), t.currency));
    }
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

    // Optional leading `^<hex>` id (git-style short hash), before the timestamp.
    // A legacy line without one parses to `id: None` (backfilled by `plc doctor`).
    let mut id = None;
    if let Some((hex, after)) = take_id(rest) {
        id = Some(hex.to_string());
        rest = after;
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
    let account = normalize_name(acct_raw);

    let rest = rest.trim_start();
    let (kind, other, mut rest) = if let Some(after_gt) = rest.strip_prefix('>') {
        let (dest_raw, after) = take_sigil_link(after_gt, '@')?;
        (Kind::Transfer, Some(normalize_name(dest_raw)), after)
    } else if let Some((cat_raw, after)) = take_sigil_link(rest, '#') {
        let kind = if neg { Kind::Expense } else { Kind::Income };
        (kind, Some(normalize_name(cat_raw)), after)
    } else {
        let kind = if neg { Kind::Expense } else { Kind::Income };
        (kind, None, rest)
    };

    // Optional balance assertion `= <±amount> [CUR]` on the account.
    let mut assert = None;
    if let Some(after_eq) = rest.trim_start().strip_prefix('=') {
        if let Some((amt_tok, after)) = next_token(after_eq) {
            if let Some((neg, mag)) = parse_amount(amt_tok) {
                assert = Some(if neg { -mag } else { mag });
                rest = match next_token(after) {
                    Some((tok, r)) if is_currency_code(tok) => r,
                    _ => after,
                };
            }
        }
    }

    // Zero or more `~[[tag]]` project tags sit between the account section and
    // the memo. (On a block head line there are none — they arrive on the
    // continuation lines handled by `parse_entries`.)
    let mut projects = Vec::new();
    while let Some(after) = take_project(rest.trim_start(), &mut projects) {
        rest = after;
    }

    let memo = rest.trim().to_string();
    let split = Vec::new(); // populated from `#[[cat]] amount` continuation lines
    Some(Transaction { id, amount, currency, kind, account, other, assert, date, state, projects, split, memo })
}

/// If `s` begins with a `~[[tag]]`, push its normalized tag onto `projects` and
/// return the remainder; else `None`.
fn take_project<'a>(s: &'a str, projects: &mut Vec<String>) -> Option<&'a str> {
    if !s.starts_with("~[[") {
        return None;
    }
    let (tag_raw, after) = take_sigil_link(s, '~')?;
    projects.push(normalize_name(tag_raw));
    Some(after)
}

/// Parse every transaction in ledger `content`, joining each block-form entry
/// (a `$` head line plus its following continuation lines) into one transaction.
/// Non-transaction lines before the first `$` (header, blanks) are skipped.
///
/// A continuation is any non-blank line that is not itself a new `$` transaction
/// — recognized **without relying on indentation**, so the block survives a
/// markdown formatter that strips leading whitespace or collapses runs of
/// spaces. A blank line or the next `$` ends the entry.
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
        // Absorb continuation lines (`#[[cat]] amount` split legs, `~[[tag]]`s,
        // then memo) until a blank line or the next `$` transaction.
        while i < lines.len()
            && !lines[i].trim().is_empty()
            && parse_line(lines[i], default_currency).is_none()
        {
            let line = lines[i].trim();
            // Balance-assertion continuation: `= <±amount> [cur]`.
            if let Some(rest) = line.strip_prefix('=') {
                if let Some((tok, _)) = next_token(rest) {
                    if let Some((neg, mag)) = parse_amount(tok) {
                        txn.assert = Some(if neg { -mag } else { mag });
                        i += 1;
                        continue;
                    }
                }
            }
            // Category continuation: `#[[cat]] amount` (a split leg) or a bare
            // `#[[cat]]` (the single category of an overflow-wrapped expense).
            if let Some((cat_raw, after)) = take_split_leg(line) {
                match next_token(after).and_then(|(tok, _)| parse_amount(tok)) {
                    Some((_, mag)) => txn.split.push((normalize_name(cat_raw), mag)),
                    None => txn.other = Some(normalize_name(cat_raw)),
                }
                i += 1;
                continue;
            }
            let mut rest = line;
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

/// A split-leg continuation `#[[cat]] amount`: return the raw category and the
/// remainder (the amount), or `None` if the line isn't a `#[[…]]` posting.
fn take_split_leg(line: &str) -> Option<(&str, &str)> {
    if !line.starts_with("#[[") {
        return None;
    }
    take_sigil_link(line, '#')
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

/// If `s` begins with a `^<hex>` id token (a git-style short hash), consume it,
/// returning the hex digits (without the `^`) and the remainder. `None` when
/// there is no leading `^`, or the token after it is not all hex digits — so a
/// stray `^` in prose is never mistaken for an id.
fn take_id(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if !s.starts_with('^') {
        return None;
    }
    let (tok, rest) = next_token(s)?;
    let hex = tok.strip_prefix('^')?;
    if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some((hex, rest))
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

/// The amount as it appears on the line: expenses `-`, income `+`, transfers a
/// bare magnitude.
fn signed_amount(t: &Transaction) -> String {
    let a = format_amount(t.amount);
    match t.kind {
        Kind::Expense => format!("-{a}"),
        Kind::Income => format!("+{a}"),
        Kind::Transfer => a,
    }
}

/// A compact one-line description of the flow, e.g. `@cash #coffee` or
/// `@checking > @cash`. Used by the register.
fn describe(t: &Transaction) -> String {
    match (t.kind, &t.other) {
        (Kind::Transfer, Some(dest)) => format!("@{} > @{}", t.account, dest),
        (_, Some(other)) => format!("@{} #{}", t.account, other),
        (_, None) => format!("@{}", t.account),
    }
}

/// Consume a `<sigil>[[inner]]` token at the start of `s` (after trimming
/// leading whitespace). Returns the **raw** inner text and the remaining string,
/// or `None` if the sigil/brackets are not there. Callers normalize (`@`/`#`
/// all via `normalize_name`).
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

/// The category-side legs of an expense/income as `(category, magnitude)`: the
/// split legs when present, else the single `other` category (or the suspense
/// bucket). Magnitudes sum to `amount`, so the book balances.
fn category_legs(t: &Transaction) -> Vec<(String, i64)> {
    if !t.split.is_empty() {
        return t.split.clone();
    }
    let cat = t.other.clone().unwrap_or_else(|| UNCATEGORIZED.to_string());
    vec![(cat, t.amount)]
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
                // category/categories (destination): -total on the account,
                // +leg on each category → nets to zero. Splits distribute across
                // several categories; a missing category → suspense bucket.
                *cur.accounts.entry(t.account.clone()).or_default() -= t.amount;
                for (cat, amt) in category_legs(t) {
                    *cur.categories.entry(cat).or_default() += amt;
                }
            }
            Kind::Income => {
                cur.income += t.amount;
                // Money leaves the income category/categories (source) and lands
                // in the account (destination).
                *cur.accounts.entry(t.account.clone()).or_default() += t.amount;
                for (cat, amt) in category_legs(t) {
                    *cur.categories.entry(cat).or_default() -= amt;
                }
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
    /// Report display only (ignored by the register): cap the account/category/
    /// tag trees at this many levels, folding deeper nodes into their ancestor.
    pub depth: Option<usize>,
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
pub fn report(root: &Path, default_currency: &str, filter: &Filter) -> Result<String, String> {
    let (items, ledger_files) = collect(root, default_currency, filter)?;
    let txns: Vec<Transaction> = items.into_iter().map(|(_, t)| t).collect();
    Ok(render(&summarize(&txns), ledger_files, filter.depth))
}

/// A chronological register of the matching transactions with a per-currency
/// running total of net-worth change (income `+`, expense `-`, transfers net 0),
/// in the style of ledger's `reg`.
pub fn register(root: &Path, default_currency: &str, filter: &Filter) -> Result<String, String> {
    let (mut items, _) = collect(root, default_currency, filter)?;
    // Sort by effective date; ties keep file/line order (sort is stable).
    items.sort_by(|a, b| a.0.cmp(&b.0));

    let mut lines = vec![String::new(), format!("  Register — {} transaction(s)", items.len())];
    if items.is_empty() {
        lines.push(String::new());
        lines.push("  (no transactions found)".to_string());
        return Ok(lines.join("\n"));
    }
    lines.push(String::new());
    let mut running: BTreeMap<String, i64> = BTreeMap::new();
    for (eff, t) in &items {
        let delta = match t.kind {
            Kind::Expense => -t.amount,
            Kind::Income => t.amount,
            Kind::Transfer => 0,
        };
        let run = running.entry(t.currency.clone()).or_default();
        *run += delta;
        let date = eff.map_or_else(|| "----------".to_string(), |d| d.format("%Y-%m-%d").to_string());
        let memo = if t.memo.is_empty() { String::new() } else { format!("  {}", t.memo) };
        lines.push(format!(
            "  {date}  {:>11} {:<3} {:>12}  {}{memo}",
            signed_amount(t),
            t.currency,
            format_signed(*run),
            describe(t),
        ));
    }
    Ok(lines.join("\n"))
}

/// Recent transactions as `(path, entry-block)` pairs, oldest→newest, capped at
/// `cap`. `path` is relative to `root`; `entry` is the canonical [`format_entry`]
/// block as it appears in the file. Ordered by the transaction instant (bare-day
/// entries first). Backs the always-current `.plc/last-transactions` cache and,
/// through it, `plc ledger last` and `undo`.
pub fn recent_entries(root: &Path, default_currency: &str, cap: usize) -> Result<Vec<(String, String)>, String> {
    if !root.is_dir() {
        return Err(format!("ledger: cannot read {}", root.display()));
    }
    let mut rows: Vec<(Option<DateTime<FixedOffset>>, String, String)> = Vec::new();
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
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        for t in parse_entries(&content, default_currency) {
            rows.push((t.date, rel.clone(), format_entry(&t)));
        }
    }
    // Stable sort by instant keeps file/parse order within the same timestamp.
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let start = rows.len().saturating_sub(cap);
    Ok(rows[start..].iter().map(|(_, p, e)| (p.clone(), e.clone())).collect())
}

/// A compact balance snapshot: per-currency net worth, income/expense/net, and
/// non-zero account balances, followed by the most recent `recent` transactions
/// (newest first). Honors the same filter as report/register — a quick "where do
/// I stand + what did I just do" view.
pub fn balance(
    root: &Path,
    default_currency: &str,
    filter: &Filter,
    recent: usize,
) -> Result<String, String> {
    let (mut items, _) = collect(root, default_currency, filter)?;
    items.sort_by(|a, b| a.0.cmp(&b.0));

    let mut lines = vec![String::new(), format!("  Balance — {} transaction(s)", items.len())];
    if items.is_empty() {
        lines.push(String::new());
        lines.push("  (no transactions found)".to_string());
        return Ok(lines.join("\n"));
    }

    let txns: Vec<Transaction> = items.iter().map(|(_, t)| t.clone()).collect();
    for (cur, t) in &summarize(&txns) {
        let net_worth: i64 = t.accounts.values().sum();
        lines.push(String::new());
        lines.push(format!("  {cur}"));
        lines.push(format!("    net worth : {}", format_signed(net_worth)));
        lines.push(format!(
            "    in {}  out {}  net {}",
            format_signed(t.income),
            format_signed(-t.expense),
            format_signed(t.net()),
        ));
        push_section(&mut lines, "accounts", &t.accounts, None, true);
    }

    // The last `recent` transactions, newest first — a short "what just happened".
    lines.push(String::new());
    let shown = recent.min(items.len());
    lines.push(format!("    last {shown}"));
    for (eff, t) in items.iter().rev().take(recent) {
        let date = eff.map_or_else(|| "----------".to_string(), |d| d.format("%Y-%m-%d").to_string());
        let memo = if t.memo.is_empty() { String::new() } else { format!("  {}", t.memo) };
        lines.push(format!(
            "    {date}  {:>11} {:<3}  {}{memo}",
            signed_amount(t),
            t.currency,
            describe(t),
        ));
    }
    Ok(lines.join("\n"))
}

/// Reformat every `*+ledger.md` file under `root` in place: re-parse each
/// transaction and re-render it via [`format_entry`] (canonical spacing, memo on
/// its own line, [`MAX_LINE`]-col wrapping), preserving each file's header. With
/// `check`, report what *would* change without writing. Returns a summary.
pub fn fmt(root: &Path, default_currency: &str, check: bool) -> Result<String, String> {
    if !root.is_dir() {
        return Err(format!("ledger: cannot read {}", root.display()));
    }
    let mut seen = 0usize;
    let mut changed: Vec<String> = Vec::new();
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
        seen += 1;
        let formatted = reformat_content(&content, default_currency);
        if formatted != content {
            changed.push(path.display().to_string());
            if !check {
                fs::write(path, &formatted).map_err(|e| format!("ledger fmt: {}: {e}", path.display()))?;
            }
        }
    }
    if changed.is_empty() {
        return Ok(format!("  ledger fmt — {seen} ledger file(s) already formatted  OK"));
    }
    let verb = if check { "would reformat" } else { "reformatted" };
    let mut lines = vec![format!("  ledger fmt — {verb} {}/{seen} ledger file(s)", changed.len())];
    lines.extend(changed.iter().take(20).map(|p| format!("    {p}")));
    if changed.len() > 20 {
        lines.push(format!("    … and {} more", changed.len() - 20));
    }
    Ok(lines.join("\n"))
}

/// Rewrite one ledger file's text: keep the header (everything up to the first
/// transaction line), then re-render every transaction canonically. A file with
/// no transactions is returned unchanged.
fn reformat_content(content: &str, default_currency: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let Some(first) = lines.iter().position(|l| parse_line(l, default_currency).is_some()) else {
        return content.to_string();
    };
    // Header = lines before the first transaction, trailing blanks trimmed.
    let mut end = first;
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let header = lines[..end].join("\n");
    let body = lines[first..].join("\n");
    let rendered: Vec<String> = parse_entries(&body, default_currency).iter().map(format_entry).collect();
    format!("{header}\n\n{}\n", rendered.join("\n"))
}

/// Backfill a stable [`txn_id`] onto every transaction that lacks one across all
/// `*+ledger.md` files under `root`, seeding it from content and re-rendering the
/// file canonically (existing ids are preserved verbatim — never re-seeded). With
/// `apply`, write the changed files; otherwise this is a dry run. Returns the
/// number of ids assigned. Mirrors [`fmt`]; backs `plc doctor`'s id repair.
pub fn backfill_ids(root: &Path, default_currency: &str, apply: bool) -> Result<usize, String> {
    if !root.is_dir() {
        return Err(format!("ledger: cannot read {}", root.display()));
    }
    let mut assigned = 0usize;
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
        let (new_content, count) = seed_missing_ids(&content, default_currency);
        if count > 0 {
            assigned += count;
            if apply && new_content != content {
                fs::write(path, &new_content)
                    .map_err(|e| format!("ledger backfill: {}: {e}", path.display()))?;
            }
        }
    }
    Ok(assigned)
}

/// Rewrite one ledger file's text with a fresh id on each id-less transaction,
/// returning `(new_text, ids_assigned)`. Like [`reformat_content`] but stamps a
/// [`txn_id`] onto every entry whose `id` is `None` before re-rendering.
fn seed_missing_ids(content: &str, default_currency: &str) -> (String, usize) {
    let lines: Vec<&str> = content.lines().collect();
    let Some(first) = lines.iter().position(|l| parse_line(l, default_currency).is_some()) else {
        return (content.to_string(), 0);
    };
    let mut end = first;
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let header = lines[..end].join("\n");
    let body = lines[first..].join("\n");
    let mut assigned = 0usize;
    let rendered: Vec<String> = parse_entries(&body, default_currency)
        .into_iter()
        .map(|mut t| {
            if t.id.is_none() {
                t.id = Some(txn_id(&t));
                assigned += 1;
            }
            format_entry(&t)
        })
        .collect();
    (format!("{header}\n\n{}\n", rendered.join("\n")), assigned)
}

/// Ids shared by more than one transaction across all ledgers (a handle should
/// be unique). Sorted, de-duplicated. Empty when every id is distinct. Not
/// auto-fixable — `plc doctor` reports these for a manual edit.
pub fn duplicate_ids(root: &Path, default_currency: &str) -> Result<Vec<String>, String> {
    let (items, _) = collect(root, default_currency, &Filter::default())?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, t) in &items {
        if let Some(id) = &t.id {
            *counts.entry(id.clone()).or_default() += 1;
        }
    }
    Ok(counts.into_iter().filter(|(_, n)| *n > 1).map(|(id, _)| id).collect())
}

/// Every transaction whose id begins with `prefix` (a git-style abbreviation),
/// paired with its ledger file path relative to `root`. Empty when none match;
/// more than one means the prefix is ambiguous (or a duplicate id). `prefix` is
/// matched case-insensitively against the stored lowercase-hex id. Backs
/// `plc ledger edit`'s id lookup.
pub fn find_by_id(root: &Path, default_currency: &str, prefix: &str) -> Result<Vec<(String, Transaction)>, String> {
    if !root.is_dir() {
        return Err(format!("ledger: cannot read {}", root.display()));
    }
    let needle = ascii_lower(prefix);
    let mut out = Vec::new();
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
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        for t in parse_entries(&content, default_currency) {
            if t.id.as_deref().is_some_and(|id| id.starts_with(&needle)) {
                out.push((rel.clone(), t));
            }
        }
    }
    Ok(out)
}

/// Rewrite the ledger file at `path`, replacing the transaction whose id equals
/// `id` with `replacement` (or dropping it when `None`), then re-rendering the
/// file canonically (header preserved, entry order kept). Returns `true` if the
/// id was found. Re-parses first, so it is robust to non-canonical input like
/// [`fmt`]. Backs `plc ledger edit`'s in-place rewrite.
pub fn rewrite_txn(
    path: &Path,
    default_currency: &str,
    id: &str,
    replacement: Option<&Transaction>,
) -> Result<bool, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("ledger edit: {}: {e}", path.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    let Some(first) = lines.iter().position(|l| parse_line(l, default_currency).is_some()) else {
        return Ok(false);
    };
    let mut end = first;
    while end > 0 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let header = lines[..end].join("\n");
    let body = lines[first..].join("\n");
    let mut found = false;
    let mut rendered: Vec<String> = Vec::new();
    for t in parse_entries(&body, default_currency) {
        if t.id.as_deref() == Some(id) {
            found = true;
            if let Some(new) = replacement {
                rendered.push(format_entry(new));
            }
        } else {
            rendered.push(format_entry(&t));
        }
    }
    if !found {
        return Ok(false);
    }
    let new_content = if rendered.is_empty() {
        format!("{header}\n")
    } else {
        format!("{header}\n\n{}\n", rendered.join("\n"))
    };
    fs::write(path, &new_content).map_err(|e| format!("ledger edit: {}: {e}", path.display()))?;
    Ok(true)
}

/// A transaction paired with its effective date (its own timestamp, or the
/// ledger file's day when it carries none).
type Dated = (Option<NaiveDate>, Transaction);

/// Verify every balance assertion (`… @[[acct]] = X`) across all ledgers: replay
/// transactions in date order, tracking each `@` account's running balance per
/// currency, and check that the asserted balance matches after the asserting
/// transaction. `Ok` with a count when all pass; `Err` listing the mismatches.
pub fn check(
    root: &Path,
    default_currency: &str,
    strict: bool,
    declared_accounts: &[String],
    declared_categories: &[String],
) -> Result<String, String> {
    let (mut items, _) = collect(root, default_currency, &Filter::default())?;
    items.sort_by(|a, b| a.0.cmp(&b.0));

    let mut bal: BTreeMap<(String, String), i64> = BTreeMap::new();
    let mut checked = 0usize;
    let mut fails: Vec<String> = Vec::new();
    if strict {
        fails.extend(undeclared(root, &items, declared_accounts, declared_categories));
    }
    for (eff, t) in &items {
        let key = |acct: &str| (t.currency.clone(), acct.to_string());
        match t.kind {
            Kind::Expense => *bal.entry(key(&t.account)).or_default() -= t.amount,
            Kind::Income => *bal.entry(key(&t.account)).or_default() += t.amount,
            Kind::Transfer => {
                *bal.entry(key(&t.account)).or_default() -= t.amount;
                if let Some(dest) = &t.other {
                    *bal.entry(key(dest)).or_default() += t.amount;
                }
            }
        }
        if let Some(expected) = t.assert {
            checked += 1;
            let actual = *bal.get(&key(&t.account)).unwrap_or(&0);
            if actual != expected {
                let date = eff.map_or_else(|| "----------".to_string(), |d| d.format("%Y-%m-%d").to_string());
                fails.push(format!(
                    "  {date}  @{}: expected {} {}, got {}",
                    t.account,
                    format_signed(expected),
                    t.currency,
                    format_signed(actual),
                ));
            }
        }
    }
    if !fails.is_empty() {
        let mut msg = vec![format!("ledger: {} check(s) failed:", fails.len())];
        msg.extend(fails);
        return Err(msg.join("\n"));
    }
    Ok(format!("  {checked} balance assertion(s) OK"))
}

/// Declared ledger names, gathered from `account`/`category`/`commodity`
/// directive lines (column 0) in any ledger file.
#[derive(Default)]
struct Declarations {
    accounts: HashSet<String>,
    categories: HashSet<String>,
    commodities: HashSet<String>,
}

/// Scan every ledger file for declaration directives.
fn scan_declarations(root: &Path) -> Declarations {
    let mut d = Declarations::default();
    for entry in WalkDir::new(root).into_iter().flatten() {
        let path = entry.path();
        if !path.file_name().and_then(|s| s.to_str()).is_some_and(|n| n.ends_with("+ledger.md")) {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else { continue };
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("account ") {
                d.accounts.insert(normalize_name(rest));
            } else if let Some(rest) = line.strip_prefix("category ") {
                d.categories.insert(normalize_name(rest));
            } else if let Some(rest) = line.strip_prefix("commodity ") {
                d.commodities.insert(rest.trim().to_string());
            }
        }
    }
    d
}

/// Under `--strict`: names used by transactions but never declared (likely
/// typos), as sorted diagnostic lines. Declarations come from in-file directive
/// lines plus the caller's `.plc/config` sets. Empty when everything checks out.
fn undeclared(root: &Path, items: &[Dated], extra_accounts: &[String], extra_categories: &[String]) -> Vec<String> {
    let mut d = scan_declarations(root);
    d.accounts.extend(extra_accounts.iter().cloned());
    d.categories.extend(extra_categories.iter().cloned());
    let (mut accts, mut cats, mut curs) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for (_, t) in items {
        accts.insert(t.account.clone());
        curs.insert(t.currency.clone());
        match t.kind {
            Kind::Transfer => {
                if let Some(dest) = &t.other {
                    accts.insert(dest.clone());
                }
            }
            _ if !t.split.is_empty() => cats.extend(t.split.iter().map(|(c, _)| c.clone())),
            _ => {
                if let Some(c) = &t.other {
                    cats.insert(c.clone());
                }
            }
        }
    }
    let mut out = Vec::new();
    out.extend(accts.iter().filter(|a| !d.accounts.contains(*a)).map(|a| format!("  undeclared account: @{a}")));
    out.extend(cats.iter().filter(|c| !d.categories.contains(*c)).map(|c| format!("  undeclared category: #{c}")));
    out.extend(curs.iter().filter(|c| !d.commodities.contains(*c)).map(|c| format!("  undeclared commodity: {c}")));
    out
}

/// Every account and category name actually used across all ledgers, sorted and
/// de-duplicated (accounts, categories). The `uncategorized` suspense bucket is
/// excluded. Used by `plc ledger acct/cat --import` to seed `.plc/config`.
pub fn names(root: &Path, default_currency: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let (items, _) = collect(root, default_currency, &Filter::default())?;
    let txns: Vec<Transaction> = items.into_iter().map(|(_, t)| t).collect();
    let (mut accts, mut cats) = (BTreeSet::new(), BTreeSet::new());
    for t in summarize(&txns).values() {
        accts.extend(t.accounts.keys().cloned());
        cats.extend(t.categories.keys().filter(|c| c.as_str() != UNCATEGORIZED).cloned());
    }
    Ok((accts.into_iter().collect(), cats.into_iter().collect()))
}

/// Distinct currencies used across all ledgers, each with its transaction count,
/// sorted by code. Lets `plc ledger doctor` spot a missing / mismatched default.
pub fn currencies(root: &Path, default_currency: &str) -> Result<Vec<(String, usize)>, String> {
    let (items, _) = collect(root, default_currency, &Filter::default())?;
    let txns: Vec<Transaction> = items.into_iter().map(|(_, t)| t).collect();
    Ok(summarize(&txns).into_iter().map(|(c, t)| (c, t.count)).collect())
}

/// Walk `root` for `*+ledger.md` files and return every transaction that passes
/// `filter`, paired with its effective date, plus the count of ledger files seen.
fn collect(root: &Path, default_currency: &str, filter: &Filter) -> Result<(Vec<Dated>, usize), String> {
    if !root.is_dir() {
        return Err(format!("ledger: cannot read {}", root.display()));
    }
    let mut items: Vec<Dated> = Vec::new();
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
        let fday = file_day(name);
        for t in parse_entries(&content, default_currency) {
            let eff = t.date.map(|d| d.date_naive()).or(fday);
            if filter.matches(&t, eff) {
                items.push((eff, t));
            }
        }
    }
    Ok((items, ledger_files))
}

/// The day encoded in a `YYYY-MM-DD+ledger.md` filename, if present.
fn file_day(name: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(name.get(..10)?, "%Y-%m-%d").ok()
}

/// Which per-day quantity `daily_spend` sums.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measure {
    /// Total spent (expense magnitude). The default for `fin stat`.
    Expense,
    /// Total received (income).
    Income,
    /// Magnitude of the day's net flow (income − expense).
    Net,
}

/// Per-day totals of `measure` (minor units) for `fin stat`, in `currency`, for
/// month `(year, Some(m))` (length = days in month) or a whole year `(year,
/// None)` (length = days in year, day-of-year order — matches `collect_year`).
/// Honors `filter`; a transaction lands on its effective date. Transfers and
/// other currencies are ignored.
pub fn daily_spend(
    root: &Path,
    currency: &str,
    filter: &Filter,
    year: i32,
    month: Option<u32>,
    measure: Measure,
) -> Result<Vec<u64>, String> {
    let (items, _) = collect(root, currency, filter)?;
    let len = match month {
        Some(m) => last_day_of_month(year, m) as usize,
        None => (1..=12).map(|m| last_day_of_month(year, m) as usize).sum(),
    };
    let mut vals = vec![0i64; len];
    for (eff, t) in &items {
        if t.currency != currency {
            continue;
        }
        let Some(d) = eff else { continue };
        if d.year() != year {
            continue;
        }
        let idx = match month {
            Some(m) if d.month() == m => (d.day() - 1) as usize,
            Some(_) => continue,
            None => (d.ordinal() - 1) as usize,
        };
        vals[idx] += match (measure, t.kind) {
            (Measure::Expense, Kind::Expense) => t.amount,
            (Measure::Income, Kind::Income) => t.amount,
            (Measure::Net, Kind::Expense) => -t.amount,
            (Measure::Net, Kind::Income) => t.amount,
            _ => 0, // transfers, or a kind the measure ignores
        };
    }
    Ok(vals.into_iter().map(i64::unsigned_abs).collect())
}

/// Format a summary as a human-readable block, in the visual style of
/// `orphans::report` (leading blank line, indented, no trailing newline).
fn render(summary: &BTreeMap<String, CurrencyTotals>, ledger_files: usize, depth: Option<usize>) -> String {
    let total: usize = summary.values().map(|c| c.count).sum();
    let mut lines: Vec<String> = Vec::new();
    lines.push(String::new());
    lines.push(format!(
        "  Ledger — {total} transaction(s) across {ledger_files} ledger file(s)"
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
            "0.00  OK".to_string()
        } else {
            format!("{}  !! UNBALANCED", format_signed(residual))
        };
        lines.push(format!("    book     : {book}"));
        // Accounts hide zero balances (a settled/closed account is just noise);
        // categories/projects show every non-empty row.
        push_section(&mut lines, "by account", &t.accounts, depth, true);
        push_section(&mut lines, "by category", &t.categories, depth, false);
        push_section(&mut lines, "by project", &t.projects, depth, false);
    }
    lines.join("\n")
}

/// Append a titled section, rolling `/`-nested names up into a tree (each parent
/// summing its descendants) capped at `depth` levels. Skipped when empty.
fn push_section(
    lines: &mut Vec<String>,
    title: &str,
    rows: &BTreeMap<String, i64>,
    depth: Option<usize>,
    hide_zero: bool,
) {
    if rows.is_empty() {
        return;
    }
    let entries: Vec<(usize, String, i64)> = rollup(rows, depth)
        .into_iter()
        .filter(|(_, _, total)| !(hide_zero && *total == 0))
        .collect();
    if entries.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("    {title}"));
    for (level, label, total) in entries {
        let name = format!("{}{label}", "  ".repeat(level));
        lines.push(format!("      {name:<18}{}", format_signed(total)));
    }
}

/// Roll a flat `name → total` map up its `/` hierarchy: every ancestor prefix
/// accumulates its descendants' totals. Returns `(level, leaf_label, total)` in
/// tree order (BTreeMap sorts parents before children), dropping nodes deeper
/// than `depth` levels (their totals already live in the retained ancestor).
fn rollup(rows: &BTreeMap<String, i64>, depth: Option<usize>) -> Vec<(usize, String, i64)> {
    let mut totals: BTreeMap<String, i64> = BTreeMap::new();
    for (name, v) in rows {
        let segs: Vec<&str> = name.split('/').collect();
        for i in 1..=segs.len() {
            *totals.entry(segs[..i].join("/")).or_default() += *v;
        }
    }
    totals
        .into_iter()
        .filter_map(|(path, v)| {
            let level = path.matches('/').count();
            if depth.is_some_and(|d| level + 1 > d) {
                return None;
            }
            let label = path.rsplit('/').next().unwrap_or(&path).to_string();
            Some((level, label, v))
        })
        .collect()
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
            id: None,
            amount,
            currency: currency.into(),
            kind,
            account: account.into(),
            other: other.map(Into::into),
            assert: None,
            date: None,
            state: State::Uncleared,
            projects: Vec::new(),
            split: Vec::new(),
            memo: String::new(),
        }
    }

    fn expense() -> Transaction {
        Transaction {
            id: None,
            amount: 450,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "cash".into(),
            other: Some("coffee".into()),
            assert: None,
            date: None,
            state: State::Uncleared,
            projects: Vec::new(),
            split: Vec::new(),
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
            id: None,
            amount: 240000,
            currency: "EUR".into(),
            kind: Kind::Income,
            account: "checking".into(),
            other: Some("salary".into()),
            assert: None,
            date: None,
            state: State::Uncleared,
            projects: Vec::new(),
            split: Vec::new(),
            memo: "July pay".into(),
        };
        assert_eq!(format_line(&t), "$ +2400.00 EUR  @[[checking]] #[[salary]]  July pay");
        assert_eq!(parse_line(&format_line(&t), EUR).as_ref(), Some(&t));
    }

    #[test]
    fn round_trip_transfer() {
        let t = Transaction {
            id: None,
            amount: 20000,
            currency: "EUR".into(),
            kind: Kind::Transfer,
            account: "checking".into(),
            other: Some("cash".into()),
            assert: None,
            date: None,
            state: State::Uncleared,
            projects: Vec::new(),
            split: Vec::new(),
            memo: "ATM".into(),
        };
        assert_eq!(format_line(&t), "$ 200.00 EUR  @[[checking]] > @[[cash]]  ATM");
        assert_eq!(parse_line(&format_line(&t), EUR).as_ref(), Some(&t));
    }

    #[test]
    fn round_trip_no_category_no_memo() {
        let t = Transaction {
            id: None,
            amount: 1000,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "cash".into(),
            other: None,
            assert: None,
            date: None,
            state: State::Uncleared,
            projects: Vec::new(),
            split: Vec::new(),
            memo: String::new(),
        };
        assert_eq!(format_line(&t), "$ -10.00 EUR  @[[cash]]");
        assert_eq!(parse_line(&format_line(&t), EUR).as_ref(), Some(&t));
    }

    #[test]
    fn eval_amount_arithmetic() {
        assert_eq!(eval_amount("4.50"), Some(450)); // plain
        assert_eq!(eval_amount("4.50+2.20"), Some(670));
        assert_eq!(eval_amount("3*4.50"), Some(1350));
        assert_eq!(eval_amount("90/3"), Some(3000));
        assert_eq!(eval_amount("(1+2)*10"), Some(3000));
        assert_eq!(eval_amount(" 3 * 4.50 + 1 "), Some(1450)); // spaces
        assert_eq!(eval_amount("10/3"), Some(333)); // rounded to cents
        assert_eq!(eval_amount("2+3-1"), Some(400));
    }

    #[test]
    fn eval_amount_rejects_garbage() {
        assert_eq!(eval_amount(""), None);
        assert_eq!(eval_amount("--"), None);
        assert_eq!(eval_amount("2+"), None);
        assert_eq!(eval_amount("abc"), None);
        assert_eq!(eval_amount("1/0"), None);
        assert_eq!(eval_amount("(1+2"), None); // unbalanced
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
            id: None,
            amount: 450,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "cash".into(),
            other: Some("coffee".into()),
            assert: None,
            date: DateTime::parse_from_str("2026-07-18 09:30:00 +0200", TIMESTAMP_FMT).ok(),
            state: State::Cleared,
            projects: Vec::new(),
            split: Vec::new(),
            memo: "Blue Bottle".into(),
        };
        assert_eq!(
            format_line(&t),
            "$ 2026-07-18 09:30:00 +0200 * -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle"
        );
        assert_eq!(parse_line(&format_line(&t), EUR).as_ref(), Some(&t));
    }

    #[test]
    fn round_trip_with_id() {
        // The `^id` leads the line, right after `$`, and round-trips intact.
        let t = Transaction { id: Some("a1b2c3d4e5f6".into()), ..expense() };
        assert_eq!(
            format_line(&t),
            "$ ^a1b2c3d4e5f6 -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle"
        );
        assert_eq!(parse_line(&format_line(&t), EUR).as_ref(), Some(&t));
    }

    #[test]
    fn txn_id_is_deterministic_and_content_addressed() {
        let t = expense();
        let id = txn_id(&t);
        assert_eq!(id.len(), 12);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(txn_id(&t), id); // stable across calls
        // Seeding ignores any id already present (content-only).
        let with = Transaction { id: Some("ffffffffffff".into()), ..t.clone() };
        assert_eq!(txn_id(&with), id);
        // A content change yields a different id.
        let other = Transaction { amount: 999, ..t };
        assert_ne!(txn_id(&other), id);
    }

    #[test]
    fn legacy_line_has_no_id_and_reprints_identically() {
        // A line without a `^id` parses to id: None and reprints byte-identically.
        let t = parse_line("$ -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle", EUR).unwrap();
        assert_eq!(t.id, None);
        assert_eq!(format_line(&t), "$ -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle");
        // A stray caret in a memo is not mistaken for an id.
        let t = parse_line("$ -4.50 @[[cash]] #[[coffee]]  cost ^ up", EUR).unwrap();
        assert_eq!(t.id, None);
        assert_eq!(t.memo, "cost ^ up");
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
    fn fmt_rewrites_canonically_and_is_idempotent() {
        // A messy file: compact one-liners with the memo on the head and loose
        // spacing. `fmt` should re-render each entry canonically (memo below),
        // keep the header, preserve the transactions, and be idempotent.
        let dir = ledger_dir(
            "finfmt",
            "2026-07-19+ledger.md",
            "isg 2026-07-19 10:00:00 +0200\n\n[[ledger]]\n\n\
             $ -11.00 EUR   @[[revolut]] #[[food/out]]   takos\n\
             $ -2.20 EUR @[[cash]] #[[savings]]  cash balance\n",
        );
        let file = dir.join("2026/07/2026-07-19+ledger.md");

        let out = fmt(&dir, EUR, false).unwrap();
        assert!(out.contains("reformatted"), "{out}");
        let after = fs::read_to_string(&file).unwrap();
        assert!(after.contains("[[ledger]]"), "header dropped: {after}");
        // Memo dropped to its own indented line; account + category stay on head.
        assert!(after.contains("@[[revolut]] #[[food/out]]\n    takos"), "{after}");
        assert!(after.contains("@[[cash]] #[[savings]]\n    cash balance"), "{after}");
        // Transactions survive the round-trip unchanged.
        assert_eq!(parse_entries(&after, EUR).len(), 2);

        // Second run: nothing left to change.
        let again = fmt(&dir, EUR, false).unwrap();
        assert!(again.contains("already formatted"), "not idempotent: {again}");
        assert_eq!(fs::read_to_string(&file).unwrap(), after);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fmt_preserves_an_existing_id() {
        // An entry that already carries a `^id` keeps it verbatim through fmt
        // (fmt reformats but never re-seeds or drops the frozen handle).
        let dir = ledger_dir(
            "finfmtid",
            "2026-07-19+ledger.md",
            "isg 2026-07-19 10:00:00 +0200\n\n[[ledger]]\n\n\
             $ ^a1b2c3d4e5f6 -4.50 EUR   @[[cash]] #[[coffee]]   latte\n",
        );
        let file = dir.join("2026/07/2026-07-19+ledger.md");
        fmt(&dir, EUR, false).unwrap();
        let after = fs::read_to_string(&file).unwrap();
        assert!(after.contains("$ ^a1b2c3d4e5f6 "), "id dropped: {after}");
        assert_eq!(parse_entries(&after, EUR)[0].id.as_deref(), Some("a1b2c3d4e5f6"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn backfill_ids_assigns_missing_and_is_idempotent() {
        // Two legacy (id-less) entries; one already carries an id and must be
        // left untouched.
        let dir = ledger_dir(
            "finbackfill",
            "2026-07-19+ledger.md",
            "isg 2026-07-19 10:00:00 +0200\n\n[[ledger]]\n\n\
             $ ^deadbeef0001 -1.00 EUR  @[[cash]] #[[tea]]\n\
             $ -4.50 EUR  @[[cash]] #[[coffee]]\n\
             $ -9.00 EUR  @[[cash]] #[[lunch]]\n",
        );
        let file = dir.join("2026/07/2026-07-19+ledger.md");

        // Dry run reports the two missing without writing (entries stay id-less).
        assert_eq!(backfill_ids(&dir, EUR, false).unwrap(), 2);
        let before = parse_entries(&fs::read_to_string(&file).unwrap(), EUR);
        assert_eq!((before[1].id.as_deref(), before[2].id.as_deref()), (None, None));

        // Apply assigns exactly the two missing; the pre-existing id survives.
        assert_eq!(backfill_ids(&dir, EUR, true).unwrap(), 2);
        let txns = parse_entries(&fs::read_to_string(&file).unwrap(), EUR);
        assert_eq!(txns[0].id.as_deref(), Some("deadbeef0001"));
        assert!(txns[1].id.as_deref().is_some_and(|id| id.len() == 12));
        assert!(txns[2].id.as_deref().is_some_and(|id| id.len() == 12));

        // Second apply is a no-op (nothing left missing).
        assert_eq!(backfill_ids(&dir, EUR, true).unwrap(), 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_ids_detects_a_clash() {
        let dir = ledger_dir(
            "findupids",
            "2026-07-19+ledger.md",
            "isg 2026-07-19 10:00:00 +0200\n\n[[ledger]]\n\n\
             $ ^cafef00d1234 -1.00 EUR  @[[cash]] #[[tea]]\n\
             $ ^cafef00d1234 -2.00 EUR  @[[cash]] #[[coffee]]\n\
             $ ^0000feedbead -3.00 EUR  @[[cash]] #[[lunch]]\n",
        );
        assert_eq!(duplicate_ids(&dir, EUR).unwrap(), vec!["cafef00d1234".to_string()]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_by_id_matches_unique_and_ambiguous_prefixes() {
        let dir = ledger_dir(
            "finfind",
            "2026-07-19+ledger.md",
            "isg 2026-07-19 10:00:00 +0200\n\n[[ledger]]\n\n\
             $ ^abc111000000 -1.00 EUR  @[[cash]] #[[tea]]\n\
             $ ^abc222000000 -2.00 EUR  @[[cash]] #[[coffee]]\n\
             $ ^def333000000 -3.00 EUR  @[[cash]] #[[lunch]]\n",
        );
        // A unique prefix resolves to exactly one; case-insensitive.
        let one = find_by_id(&dir, EUR, "DEF").unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].1.other.as_deref(), Some("lunch"));
        // An ambiguous prefix returns every match.
        assert_eq!(find_by_id(&dir, EUR, "abc").unwrap().len(), 2);
        // No match → empty.
        assert!(find_by_id(&dir, EUR, "999").unwrap().is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rewrite_txn_replaces_and_drops_by_id() {
        let dir = ledger_dir(
            "finrewrite",
            "2026-07-19+ledger.md",
            "isg 2026-07-19 10:00:00 +0200\n\n[[ledger]]\n\n\
             $ ^aaa000000000 -1.00 EUR  @[[cash]] #[[tea]]\n\
             $ ^bbb000000000 -2.00 EUR  @[[cash]] #[[coffee]]\n",
        );
        let file = dir.join("2026/07/2026-07-19+ledger.md");

        // Replace: swap the coffee entry's amount/memo (keeping its id).
        let mut edited = find_by_id(&dir, EUR, "bbb").unwrap().pop().unwrap().1;
        edited.amount = 500;
        edited.memo = "latte".into();
        assert!(rewrite_txn(&file, EUR, "bbb000000000", Some(&edited)).unwrap());
        let after = fs::read_to_string(&file).unwrap();
        assert!(after.contains("$ ^bbb000000000 -5.00 EUR  @[[cash]] #[[coffee]]\n    latte"), "{after}");
        assert!(after.contains("^aaa000000000"), "sibling kept: {after}");

        // Drop: remove the tea entry.
        assert!(rewrite_txn(&file, EUR, "aaa000000000", None).unwrap());
        let after = fs::read_to_string(&file).unwrap();
        assert!(!after.contains("^aaa000000000") && after.contains("^bbb000000000"), "{after}");

        // Unknown id → not found, no change.
        assert!(!rewrite_txn(&file, EUR, "ffffffffffff", None).unwrap());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hierarchical_categories_roll_up_with_depth() {
        let dir = ledger_dir(
            "finhier",
            "2026-07-19+ledger.md",
            "$ -60 EUR @[[bank/checking]] #[[food/groceries]]\n\
             $ -25 EUR @[[bank/checking]] #[[food/dining]]\n\
             $ -900 EUR @[[bank/checking]] #[[rent]]\n",
        );
        // Full depth: parent `food` sums its children, which also appear.
        let full = report(&dir, EUR, &Filter::default()).unwrap();
        assert!(full.contains("food") && full.contains("groceries") && full.contains("dining"));
        // The `food` parent totals +85.00 (60 + 25).
        assert!(full.contains("+85.00"), "{full}");

        // depth 1: only top-level nodes; children folded into the parent total.
        let shallow = Filter { depth: Some(1), ..Filter::default() };
        let out = report(&dir, EUR, &shallow).unwrap();
        assert!(out.contains("food") && out.contains("+85.00"), "{out}");
        assert!(!out.contains("groceries") && !out.contains("dining"), "{out}");
        fs::remove_dir_all(&dir).ok();
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
    fn register_lists_in_date_order_with_running_total() {
        // Entries out of date order in the file; register sorts and accumulates.
        let dir = ledger_dir(
            "finreg",
            "2026-07-19+ledger.md",
            "$ 2026-07-18 00:00:00 +0200 -4.50 EUR @[[cash]] #[[coffee]]\n\
             $ 2026-07-01 00:00:00 +0200 +2000 EUR @[[bnp]] #[[salary]]\n\
             $ 2026-07-10 00:00:00 +0200 200 EUR @[[bnp]] > @[[cash]]\n",
        );
        let out = register(&dir, EUR, &Filter::default()).unwrap();
        let dates: Vec<&str> = out
            .lines()
            .filter_map(|l| l.trim().split(' ').next().filter(|d| d.starts_with("2026")))
            .collect();
        assert_eq!(dates, ["2026-07-01", "2026-07-10", "2026-07-18"], "sorted: {out}");
        // Running net worth: +2000 (salary), +2000 (transfer nets 0), 1995.50 (coffee).
        assert!(out.contains("+2000.00") && out.contains("+1995.50"), "{out}");
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
    fn split_block_round_trips_and_balances() {
        // A split expense: card pays 90, distributed across three categories.
        // Canonical block form: total on the head, split legs then memo indented.
        let content = "$ 2026-07-19 00:00:00 +0200 -90.00 EUR  @[[card]]\n\
                       \x20   #[[food]]  -60.00 EUR\n\
                       \x20   #[[household]]  -25.00 EUR\n\
                       \x20   #[[tax]]  -5.00 EUR\n\
                       \x20   Costco";
        let txns = parse_entries(content, EUR);
        assert_eq!(txns.len(), 1);
        let t = &txns[0];
        assert_eq!(t.amount, 9000);
        assert_eq!(t.account, "card");
        assert_eq!(t.other, None);
        assert_eq!(t.memo, "Costco");
        assert_eq!(
            t.split,
            vec![("food".into(), 6000), ("household".into(), 2500), ("tax".into(), 500)]
        );
        // Round-trips to the same block.
        assert_eq!(format_entry(t), content);
        // Summed across categories, the book still balances.
        let s = summarize(&txns);
        let eur = &s["EUR"];
        assert_eq!(eur.categories["food"], 6000);
        assert_eq!(eur.categories["household"], 2500);
        assert_eq!(eur.accounts["card"], -9000);
        assert_eq!(eur.residual(), 0);
    }

    #[test]
    fn split_keeps_tags_on_the_head_line() {
        // A split with a `~` tag: the tag trails the account on the head line
        // (not a separate continuation line), and the block round-trips.
        let t = Transaction {
            id: None,
            amount: 9000,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "cash".into(),
            other: None,
            assert: None,
            date: None,
            state: State::Uncleared,
            projects: vec!["japan-trip/work".into()],
            split: vec![("food".into(), 6000), ("household".into(), 3000)],
            memo: "Costco".into(),
        };
        let block = format_entry(&t);
        assert_eq!(
            block,
            "$ -90.00 EUR  @[[cash]] ~[[japan-trip/work]]\n\
             \x20   #[[food]]  -60.00 EUR\n\
             \x20   #[[household]]  -30.00 EUR\n\
             \x20   Costco"
        );
        assert_eq!(parse_entries(&block, EUR), vec![t]);
    }

    #[test]
    fn heavy_entry_keeps_accounting_on_head_and_round_trips() {
        // A kitchen-sink expense: full timestamp, cleared, nested account +
        // category, assertion, two tags, long memo. The accounting (account,
        // category, assertion) stays on the head — even past MAX_LINE — while
        // the tags and memo wrap onto continuation lines kept within MAX_LINE.
        let t = Transaction {
            id: None,
            amount: 4250,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "rev/eur".into(),
            other: Some("food/dining".into()),
            assert: Some(358353),
            date: DateTime::parse_from_str("2026-07-19 13:13:17 +0200", TIMESTAMP_FMT).ok(),
            state: State::Cleared,
            projects: vec!["work/q3-review".into(), "reimbursable".into()],
            split: Vec::new(),
            memo: "dinner with the whole team after the quarterly review".into(),
        };
        let block = format_entry(&t);
        let lines: Vec<&str> = block.lines().collect();
        assert!(lines[0].contains("@[[rev/eur]] #[[food/dining]]"), "acc+cat split: {}", lines[0]);
        assert!(lines[0].contains("= 3583.53 EUR"), "assertion off head: {}", lines[0]);
        for line in &lines[1..] {
            assert!(line.chars().count() <= MAX_LINE, "continuation over {MAX_LINE}: {line:?}");
        }
        assert_eq!(parse_entries(&block, EUR), vec![t]);
    }

    #[test]
    fn survives_formatter_reflow() {
        // What a 66-col markdown formatter leaves: continuation lines
        // de-indented to column 0 and the `EUR  @[[` double space collapsed.
        let reflowed = "$ 2026-07-19 13:01:56 +0200 +3694.76 EUR @[[rev/eur]] #[[opening]]\n\
                        hello world\n\
                        $ 2026-07-19 13:02:00 +0200 -21.23 EUR @[[rev/eur]] #[[food]]\n\
                        goodbye world\n";
        let txns = parse_entries(reflowed, EUR);
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].amount, 369476);
        assert_eq!(txns[0].account, "rev/eur");
        assert_eq!(txns[0].memo, "hello world"); // de-indented memo still attaches
        assert_eq!(txns[1].other.as_deref(), Some("food"));
        assert_eq!(txns[1].memo, "goodbye world");
    }

    #[test]
    fn deindented_split_block_parses() {
        let reflowed = "$ -90.00 EUR @[[card]]\n\
                        #[[food]] -60.00 EUR\n\
                        #[[household]] -30.00 EUR\n\
                        Costco\n";
        let t = &parse_entries(reflowed, EUR)[0];
        assert_eq!(t.split, vec![("food".into(), 6000), ("household".into(), 3000)]);
        assert_eq!(t.memo, "Costco");
    }

    #[test]
    fn round_trip_with_assertion() {
        let t = parse_line("$ -4.50 @[[cash]] #[[coffee]] = 480 EUR  Blue Bottle", EUR).unwrap();
        assert_eq!(t.assert, Some(48000));
        assert_eq!(t.other.as_deref(), Some("coffee"));
        assert_eq!(t.memo, "Blue Bottle");
        assert_eq!(
            format_line(&t),
            "$ -4.50 EUR  @[[cash]] #[[coffee]] = 480.00 EUR  Blue Bottle"
        );
    }

    #[test]
    fn check_passes_and_fails() {
        // cash: +200 (ATM in), then -4.50 (coffee) → 195.50 = 19550 minor.
        let good = ledger_dir(
            "finchk-ok",
            "2026-07-19+ledger.md",
            "$ 2026-07-10 00:00:00 +0200 200 EUR @[[bnp]] > @[[cash]]\n\
             $ 2026-07-18 00:00:00 +0200 -4.50 EUR @[[cash]] #[[coffee]] = 195.50 EUR\n",
        );
        assert!(check(&good, EUR, false, &[], &[]).unwrap().contains("1 balance assertion(s) OK"));
        fs::remove_dir_all(&good).ok();

        // Wrong asserted balance → error naming the account and the mismatch.
        let bad = ledger_dir(
            "finchk-bad",
            "2026-07-19+ledger.md",
            "$ 2026-07-18 00:00:00 +0200 -4.50 EUR @[[cash]] #[[coffee]] = 999 EUR\n",
        );
        let err = check(&bad, EUR, false, &[], &[]).unwrap_err();
        assert!(err.contains("failed") && err.contains("@cash"), "{err}");
        fs::remove_dir_all(&bad).ok();
    }

    #[test]
    fn daily_spend_buckets_by_day_and_filters() {
        let dir = ledger_dir(
            "finspend",
            "2026-07-19+ledger.md",
            "$ 2026-07-02 00:00:00 +0200 -4.50 EUR @[[cash]] #[[coffee]]\n\
             $ 2026-07-02 00:00:00 +0200 -12.00 EUR @[[card]] #[[food]]\n\
             $ 2026-07-05 00:00:00 +0200 -60.00 EUR @[[card]] #[[food]]\n",
        );
        // All expenses in July: day 2 = 4.50 + 12 = 16.50 (1650), day 5 = 60 (6000).
        let all = daily_spend(&dir, EUR, &Filter::default(), 2026, Some(7), Measure::Expense).unwrap();
        assert_eq!(all.len(), 31);
        assert_eq!(all[1], 1650); // 2nd
        assert_eq!(all[4], 6000); // 5th
        assert_eq!(all.iter().sum::<u64>(), 7650);

        // Filter to coffee → only the day-2 coffee expense.
        let f = Filter { patterns: vec!["coffee".into()], ..Filter::default() };
        let coffee = daily_spend(&dir, EUR, &f, 2026, Some(7), Measure::Expense).unwrap();
        assert_eq!(coffee[1], 450);
        assert_eq!(coffee.iter().sum::<u64>(), 450);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn strict_flags_undeclared_names() {
        // `cash` and EUR are declared; `coffee` is not.
        let dir = ledger_dir(
            "finstrict",
            "2026-07-19+ledger.md",
            "account cash\ncommodity EUR\n\n$ -4.50 EUR @[[cash]] #[[coffee]]\n",
        );
        // Non-strict: only assertions checked → passes.
        assert!(check(&dir, EUR, false, &[], &[]).is_ok());
        // Strict: the undeclared category is flagged.
        let err = check(&dir, EUR, true, &[], &[]).unwrap_err();
        assert!(err.contains("undeclared category: #coffee"), "{err}");
        assert!(!err.contains("undeclared account"), "cash was declared: {err}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn round_trip_with_projects() {
        let t = Transaction {
            id: None,
            amount: 8000,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "card".into(),
            other: Some("food".into()),
            assert: None,
            date: None,
            state: State::Uncleared,
            projects: vec!["trip".into()],
            split: Vec::new(),
            memo: "ramen".into(),
        };
        assert_eq!(format_line(&t), "$ -80.00 EUR  @[[card]] #[[food]] ~[[trip]]  ramen");
        // Head carries the accounting (incl. tags); only the memo drops below.
        assert_eq!(
            format_entry(&t),
            "$ -80.00 EUR  @[[card]] #[[food]] ~[[trip]]\n    ramen"
        );
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
            id: None,
            amount: 45,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "cash".into(),
            other: Some("coffee".into()),
            assert: None,
            date: None,
            state: State::Uncleared,
            projects: vec!["japan-trip/leisure".into(), "work".into()],
            split: Vec::new(),
            memo: "latte at the airport before the long flight home".into(),
        };
        let block = format_entry(&t);
        assert!(block.contains('\n'), "should wrap: {block}");
        // Head keeps account + category together; tags/memo wrap within MAX_LINE.
        let lines: Vec<&str> = block.lines().collect();
        assert!(lines[0].contains("@[[cash]] #[[coffee]]"), "acc+cat split: {}", lines[0]);
        for line in &lines[1..] {
            assert!(line.chars().count() <= MAX_LINE, "continuation over {MAX_LINE}: {line:?}");
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
    fn report_hides_zero_accounts_but_keeps_zero_categories() {
        // wallet/bank net to 0 (a round-trip transfer); refund nets to 0 too.
        let txns = vec![
            txn(450, EUR, Kind::Expense, "cash", Some("coffee")),
            txn(100, EUR, Kind::Income, "cash", Some("refund")),
            txn(100, EUR, Kind::Expense, "cash", Some("refund")),
            txn(100, EUR, Kind::Transfer, "wallet", Some("bank")),
            txn(100, EUR, Kind::Transfer, "bank", Some("wallet")),
        ];
        let out = render(&summarize(&txns), 1, None);
        assert!(out.contains("cash"), "{out}");
        assert!(!out.contains("wallet"), "zero account leaked: {out}");
        assert!(!out.contains("bank"), "zero account leaked: {out}");
        // Zero-valued categories are still listed (only accounts are pruned).
        assert!(out.contains("refund"), "zero category dropped: {out}");
    }

    #[test]
    fn balance_shows_net_worth_accounts_and_recent() {
        let dir = std::env::temp_dir().join(format!("plc-finbal-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let sub = dir.join("2026/07");
        fs::create_dir_all(&sub).unwrap();
        fs::write(
            sub.join("2026-07-19+ledger.md"),
            "isg\n\n[[ledger]]\n\n$ -4.50 EUR  @[[cash]] #[[coffee]]  Blue Bottle\n\
             $ +100 EUR @[[cash]] #[[gift]]\n\
             $ 50 EUR @[[cash]] > @[[wallet]]\n\
             $ 50 EUR @[[wallet]] > @[[cash]]\n",
        )
        .unwrap();
        let out = balance(&dir, EUR, &Filter::default(), 5).unwrap();
        assert!(out.contains("net worth : +95.50"), "{out}");
        assert!(out.contains("accounts"), "{out}");
        assert!(out.contains("cash"), "{out}");
        // The zero-balance `wallet` is pruned from the accounts section (it may
        // still appear in the recent-transactions list below it).
        let accounts_section = out.split("    last").next().unwrap();
        assert!(!accounts_section.contains("wallet"), "zero account leaked: {out}");
        assert!(out.contains("last"), "{out}");
        assert!(out.contains("Blue Bottle"), "recent memo missing: {out}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn currencies_lists_used_with_counts() {
        let dir = ledger_dir(
            "fincur",
            "2026-07-19+ledger.md",
            "$ -4.50 EUR @[[cash]] #[[coffee]]\n\
             $ -1.00 EUR @[[cash]] #[[tea]]\n\
             $ -9.00 USD @[[card]] #[[book]]\n",
        );
        assert_eq!(currencies(&dir, EUR).unwrap(), vec![("EUR".into(), 2), ("USD".into(), 1)]);
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
