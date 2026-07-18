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

use std::env;

use crate::normalize_target;

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
    let mut line = format!("$ {signed} {}  @[[{}]]", t.currency, t.account);
    match (t.kind, &t.other) {
        (Kind::Transfer, Some(dest)) => line.push_str(&format!(" > @[[{dest}]]")),
        (_, Some(cat)) => line.push_str(&format!(" #[[{cat}]]")),
        (_, None) => {}
    }
    if !t.memo.is_empty() {
        line.push_str(&format!("  {}", t.memo));
    }
    line
}

/// Parse one line into a [`Transaction`], or `None` if it is not a well-formed
/// transaction line (prose, blank, missing `$`/amount/account, malformed amount).
/// `default_currency` is used when the line carries no explicit code.
pub fn parse_line(line: &str, default_currency: &str) -> Option<Transaction> {
    let rest = line.trim_start().strip_prefix('$')?;
    // The `$` must be a standalone sigil, not the first char of some `$word`.
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }

    let (amount_tok, rest) = next_token(rest)?;
    let (neg, amount) = parse_amount(amount_tok)?;

    // An optional currency code sits between the amount and the account.
    let (currency, rest) = match next_token(rest) {
        Some((tok, after)) if is_currency_code(tok) => (tok.to_string(), after),
        _ => (default_currency.to_string(), rest),
    };

    // The account is mandatory; a line without one is not a transaction.
    let (account, rest) = take_sigil_link(rest, '@')?;

    let rest = rest.trim_start();
    let (kind, other, memo) = if let Some(after_gt) = rest.strip_prefix('>') {
        let (dest, memo) = take_sigil_link(after_gt, '@')?;
        (Kind::Transfer, Some(dest), memo.trim().to_string())
    } else if let Some((cat, memo)) = take_sigil_link(rest, '#') {
        let kind = if neg { Kind::Expense } else { Kind::Income };
        (kind, Some(cat), memo.trim().to_string())
    } else {
        let kind = if neg { Kind::Expense } else { Kind::Income };
        (kind, None, rest.trim().to_string())
    };

    Some(Transaction { amount, currency, kind, account, other, memo })
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
/// leading whitespace). Returns the normalized target and the remaining string,
/// or `None` if the sigil/brackets are not there.
fn take_sigil_link(s: &str, sigil: char) -> Option<(String, &str)> {
    let s = s.trim_start();
    let inner_and_rest = s.strip_prefix(sigil)?.strip_prefix("[[")?;
    let end = inner_and_rest.find("]]")?;
    let inner = &inner_and_rest[..end];
    let after = &inner_and_rest[end + 2..];
    Some((normalize_target(inner), after))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EUR: &str = "EUR";

    fn expense() -> Transaction {
        Transaction {
            amount: 450,
            currency: "EUR".into(),
            kind: Kind::Expense,
            account: "cash".into(),
            other: Some("coffee".into()),
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
}
