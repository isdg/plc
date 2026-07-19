//! Shared calendar-view renderers for `plc stat` and `plc fin stat`.
//!
//! These lay out per-day values (`values[i]` = day `i + 1` for a month, or
//! day-of-year for a year) as an ASCII heatmap or line chart. They are
//! *value-agnostic*: the heatmap glyph comes from an injected `symbol` fn, the
//! legend meanings from `legend`, and any numeric axis/summary label from a
//! `fmt` closure — so `plc stat` passes byte formatters and `plc fin stat`
//! passes money formatters, sharing one layout. Callers build the `values`
//! (from file sizes, or from ledger spend) and append their own stats block.

use std::fmt::Write as _;

use chrono::{Datelike, NaiveDate};
use plc_core::calendar;

/// One month as the title, a Mo–Su grid with a heatmap row under each week, and
/// the legend. `values` holds every day of the month (`len == last day`).
pub fn month_grid(y: i32, m: u32, values: &[u64], symbol: fn(u64) -> char, legend: &str) -> String {
    let first = NaiveDate::from_ymd_opt(y, m, 1).expect("first-of-month is valid");
    let first_dow = first.weekday().number_from_monday(); // Mon=1 … Sun=7
    let last_day = values.len() as u32;

    // 42-cell (6-week) Monday-first grid; cell holds the day number or 0.
    let mut cells = [0u32; 42];
    for d in 1..=last_day {
        cells[(first_dow - 1 + d - 1) as usize] = d;
    }
    let num_weeks = (first_dow - 1 + last_day).div_ceil(7);

    let mut out = String::new();
    let _ = writeln!(out, "\n              {}", first.format("%B %Y"));
    out.push_str("      Mo Tu We Th Fr Sa Su\n");
    for w in 0..num_weeks {
        out.push_str("     ");
        for dow in 0..7 {
            let d = cells[(w * 7 + dow) as usize];
            if d == 0 {
                out.push_str("   ");
            } else {
                let _ = write!(out, "{d:3}");
            }
        }
        out.push_str("\n     ");
        for dow in 0..7 {
            let d = cells[(w * 7 + dow) as usize];
            if d == 0 {
                out.push_str("   ");
            } else {
                out.push_str("  ");
                out.push(symbol(values[(d - 1) as usize]));
            }
        }
        out.push('\n');
    }
    let _ = writeln!(out, "\n     Legend:  {legend}");
    out
}

/// GitHub-style 7×weeks contribution grid for a year, with month labels above.
/// `values` is indexed by day-of-year − 1.
pub fn year_git(y: i32, values: &[u64], symbol: fn(u64) -> char, legend: &str) -> String {
    let total_days = values.len() as u32;
    let jan1_dow = NaiveDate::from_ymd_opt(y, 1, 1).unwrap().weekday().number_from_monday();
    let pad = jan1_dow - 1;
    let weeks = (pad + total_days).div_ceil(7);

    let mut grid = vec![0u32; (weeks * 7) as usize];
    let mut month_col = [0u32; 13];
    let mut doy = 0u32;
    for m in 1..=12 {
        month_col[m as usize] = (pad + doy) / 7;
        for _ in 1..=calendar::last_day_of_month(y, m) {
            doy += 1;
            grid[(pad + doy - 1) as usize] = doy;
        }
    }

    let mut out = String::new();
    out.push('\n');
    let _ = writeln!(out, "{}{y} activity", " ".repeat(39));

    out.push_str("      ");
    let mut printed = 0u32;
    for m in 1..=12 {
        let target = month_col[m as usize] * 2;
        while printed < target {
            out.push(' ');
            printed += 1;
        }
        let _ = write!(out, "{}", NaiveDate::from_ymd_opt(y, m, 1).unwrap().format("%b"));
        printed += 3;
    }
    out.push('\n');

    let dow = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    for (row, label) in dow.iter().enumerate() {
        let _ = write!(out, "  {label}  ");
        for c in 0..weeks {
            let d_idx = grid[(c * 7 + row as u32) as usize];
            if d_idx == 0 {
                out.push_str("  ");
            } else {
                out.push(symbol(values[(d_idx - 1) as usize]));
                out.push(' ');
            }
        }
        out.push('\n');
    }
    let _ = writeln!(out, "\n  Legend:  {legend}");
    out
}

/// One row per month: a heatmap strip and a right-hand `days/total` summary
/// (formatted by `fmt`); the current month is bold. `values` by day-of-year − 1.
pub fn year_tab(
    y: i32,
    values: &[u64],
    today: NaiveDate,
    symbol: fn(u64) -> char,
    legend: &str,
    fmt: &dyn Fn(u64) -> String,
) -> String {
    let mut out = String::new();
    out.push('\n');
    let _ = writeln!(out, "{}{y} activity", " ".repeat(30));
    out.push('\n');

    let mut off = 0usize;
    for m in 1..=12 {
        let last = calendar::last_day_of_month(y, m);
        let label = NaiveDate::from_ymd_opt(y, m, 1).unwrap().format("%b");
        let (b, r) = if y == today.year() && m == today.month() {
            ("\x1b[1m", "\x1b[0m")
        } else {
            ("", "")
        };
        let mut strip = String::new();
        let mut mtotal = 0u64;
        let mut mdays = 0u32;
        for d in 1..=last {
            let v = values[off + (d - 1) as usize];
            strip.push(symbol(v));
            strip.push(' ');
            if v > 0 {
                mtotal += v;
                mdays += 1;
            }
        }
        let pad = " ".repeat(62usize.saturating_sub(last as usize * 2));
        let _ = writeln!(out, "{b}  {label}  {strip}{pad}   {mdays:2}/{last}   {}{r}", fmt(mtotal));
        off += last as usize;
    }
    let _ = writeln!(out, "\n  Legend:  {legend}");
    out
}

/// A month line chart of `values` with a day-number x-axis. `unit` names the
/// quantity in the title (e.g. `bytes`, `spend`); `fmt` labels the y-axis.
pub fn plot_month(y: i32, m: u32, values: &[u64], unit: &str, fmt: &dyn Fn(u64) -> String) -> String {
    let last_day = values.len() as u32;
    let max = values.iter().copied().max().unwrap_or(0);
    let first = NaiveDate::from_ymd_opt(y, m, 1).expect("first-of-month is valid");

    let mut out = String::new();
    out.push('\n');
    let _ = writeln!(out, "          {} — daily {unit}", first.format("%B %Y"));
    out.push('\n');
    if max == 0 {
        out.push_str("          (no data)\n");
    } else {
        for line in calendar::line_chart_with(max, last_day as usize, 8, values, fmt) {
            out.push_str(&line);
            out.push('\n');
        }
        push_day_axis(&mut out, last_day);
    }
    out
}

/// A year line chart of weekly (7-day-bin) `values` with month labels on the
/// x-axis. `unit`/`fmt` as in [`plot_month`].
pub fn plot_year(y: i32, values: &[u64], unit: &str, fmt: &dyn Fn(u64) -> String) -> String {
    let nweeks = values.len().div_ceil(7);
    let mut week_sums = vec![0u64; nweeks];
    for (doy0, &v) in values.iter().enumerate() {
        week_sums[doy0 / 7] += v;
    }
    let max = week_sums.iter().copied().max().unwrap_or(0);

    let mut out = String::new();
    out.push('\n');
    let _ = writeln!(out, "          {y} — weekly {unit} (7-day bins)");
    out.push('\n');
    if max == 0 {
        out.push_str("          (no data)\n\n");
        return out;
    }
    for line in calendar::line_chart_with(max, nweeks, 8, &week_sums, fmt) {
        out.push_str(&line);
        out.push('\n');
    }

    out.push_str(&" ".repeat(11));
    let mut month_week = [0usize; 13];
    let mut cum = 0u32;
    for m in 1..=12 {
        month_week[m as usize] = (cum / 7) as usize;
        cum += calendar::last_day_of_month(y, m);
    }
    let mut printed = 0usize;
    for m in 1..=12 {
        while printed < month_week[m as usize] {
            out.push(' ');
            printed += 1;
        }
        let _ = write!(out, "{}", NaiveDate::from_ymd_opt(y, m, 1).unwrap().format("%b"));
        printed += 3;
    }
    out.push('\n');
    out
}

/// The day-number x-axis under a month plot: markers at day 1 then every 5th,
/// each column one char wide (a two-digit label consumes the next column too).
fn push_day_axis(out: &mut String, last_day: u32) {
    out.push_str(&" ".repeat(11));
    let mut d = 1;
    while d <= last_day {
        if d == 1 || d % 5 == 0 {
            let _ = write!(out, "{d}");
            if d >= 10 {
                d += 1; // the second digit occupies the next column
            }
        } else {
            out.push(' ');
        }
        d += 1;
    }
    out.push('\n');
}
