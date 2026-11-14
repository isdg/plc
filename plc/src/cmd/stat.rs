//! `plc stat` — render daily-note activity as an ASCII calendar heatmap + stats.
//!
//! Ports `_calendar.sh` (the `pst` script): scores each day by the bytes
//! written that day across the vault (see [`activity`]), lays out a
//! Sunday-first month grid with a heatmap scaled to the year, and prints a
//! Stats block (days written, total, avg, streaks, best day). Year and plot
//! modes land in follow-up commits; their flags are defined now so the CLI
//! surface stays stable.
//!
//! Note: the scope flag is `--type` (long-only) because the global `-t` is
//! taken by `--tag`; the `pst` wrapper maps a bare `-t` back to `--type`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use chrono::{Datelike, Local, NaiveDate};
use clap::Args;
use plc_core::activity;
use plc_core::calendar::{self, MonthStats, YearStats};

use crate::cmd::calview::{self, Cells, ColorArgs, Heat, Layout};
use crate::config::Palace;

/// Bytes written per day.
type Days = HashMap<NaiveDate, u64>;

/// The byte scale for a year's `values`: its levels end at the quartiles of
/// the year's written days, as GitHub scales contributions.
fn bytes_heat(values: &[u64], cells: Cells) -> Heat {
    let bounds = calendar::quartiles(values);
    let [q1, q2, q3] = bounds.map(calendar::fmt_bytes);
    let labels = [
        "empty".to_string(),
        format!("≤{q1}"),
        format!("≤{q2}"),
        format!("≤{q3}"),
        format!(">{q3}"),
    ];
    Heat { bounds, labels, cells }
}

#[derive(Args)]
pub struct StatArgs {
    /// Date as [DD [MM [YY|YYYY]]]; the day is discarded (a calendar shows a
    /// whole month or year). Any field not given here falls back to a flag,
    /// then to today.
    #[arg(value_name = "DD MM YY")]
    positional: Vec<String>,
    /// Statistic scope: `month` or `year`.
    #[arg(long = "type", value_name = "SCOPE", default_value = "month")]
    scope: String,
    /// Month 1-12 (defaults to the current month).
    #[arg(short = 'm', long = "month")]
    month: Option<String>,
    /// Year; 2-digit (25 → 2025) or 4-digit (defaults to the current year).
    #[arg(short = 'y', long = "year")]
    year: Option<String>,
    /// Heatmap layout: `git` (GitHub-style) or `tab` (a row of days).
    #[arg(
        long,
        value_enum,
        value_name = "LAYOUT",
        default_value_t = Layout::Git
    )]
    layout: Layout,
    /// Replace the heatmap with an ASCII line chart.
    #[arg(short = 'p', long = "plot")]
    plot: bool,
    /// List the notes written on one day, and the bytes each, instead.
    #[arg(
        long,
        num_args = 3,
        value_names = ["YY", "MM", "DD"],
        conflicts_with_all = ["positional", "month", "year", "plot"]
    )]
    day: Option<Vec<String>>,
    #[command(flatten)]
    color: ColorArgs,
}

pub fn run(palace: &Palace, args: StatArgs) -> Result<String, String> {
    let today = Local::now().date_naive();
    if let Some(day) = &args.day {
        return Ok(render_day(palace.root(), parse_day(day)?));
    }
    let (year, month) = resolve(args.month, args.year, &args.positional, today)?;
    let days = activity::by_day(palace.root());
    let values = calendar::year_values(&days, year);
    let heat = bytes_heat(&values, args.color.cells());
    let layout = args.layout;

    match args.scope.as_str() {
        "month" if args.plot => {
            Ok(render_plot_month(&days, year, month, today))
        }
        "month" => {
            Ok(render_month(&days, year, month, today, layout, &heat))
        }
        "year" => {
            Ok(render_year(&values, year, layout, args.plot, today, &heat))
        }
        other => Err(format!("plc stat: unknown type: {other} (expected month|year)")),
    }
}

/// Resolve the target `(year, month)` from flags + positional against `today`.
///
/// Flags apply first (each defaulting to today); positional tokens then override
/// per the script's shape — 1: `MM`, 2: `MM YY`, 3: `DD MM YY` (day discarded).
fn resolve(
    month: Option<String>,
    year: Option<String>,
    positional: &[String],
    today: NaiveDate,
) -> Result<(i32, u32), String> {
    let mut mm = match month {
        Some(s) => parse_month(&s)?,
        None => today.month(),
    };
    let mut yy = match year {
        Some(s) => parse_year(&s)?,
        None => today.year(),
    };

    match positional {
        [] => {}
        [m] => mm = parse_month(m)?,
        [m, y] => {
            mm = parse_month(m)?;
            yy = parse_year(y)?;
        }
        [_dd, m, y] => {
            mm = parse_month(m)?;
            yy = parse_year(y)?;
        }
        _ => return Err("plc stat: too many positional args".to_string()),
    }

    if !(1..=12).contains(&mm) {
        return Err(format!("plc stat: invalid month: {mm}"));
    }
    Ok((yy, mm))
}

/// Parse a base-10 month field (tolerates a leading zero, e.g. "05").
fn parse_month(s: &str) -> Result<u32, String> {
    s.trim()
        .parse::<u32>()
        .map_err(|_| format!("plc stat: invalid month: {s}"))
}

/// Parse a year, expanding an exactly-2-digit value to `20YY` (mirrors `daily`).
fn parse_year(s: &str) -> Result<i32, String> {
    let t = s.trim();
    let n: i32 = t.parse().map_err(|_| format!("plc stat: invalid year: {s}"))?;
    let two_digit = t.len() == 2 && t.bytes().all(|b| b.is_ascii_digit());
    Ok(if two_digit { 2000 + n } else { n })
}

/// Parse `--day YY MM DD`, the year as [`parse_year`] takes it.
fn parse_day(fields: &[String]) -> Result<NaiveDate, String> {
    let [y, m, d] = fields else {
        return Err("plc stat: --day takes YY MM DD".to_string());
    };
    let invalid = || format!("plc stat: invalid day: {y} {m} {d}");
    let d: u32 = d.trim().parse().map_err(|_| invalid())?;
    NaiveDate::from_ymd_opt(parse_year(y)?, parse_month(m)?, d)
        .ok_or_else(invalid)
}

/// The notes written on `day`, most bytes first, under their total.
fn render_day(root: &Path, day: NaiveDate) -> String {
    let notes = activity::on_day(root, day);
    let total: u64 = notes.iter().map(|(_, bytes)| bytes).sum();
    let date = day.format("%a %Y-%m-%d");
    let size = calendar::fmt_bytes(total);
    let mut out = match notes.len() {
        0 => format!("\n  {date}   nothing written\n"),
        1 => format!("\n  {date}   {size} in 1 note\n\n"),
        n => format!("\n  {date}   {size} in {n} notes\n\n"),
    };
    for (path, bytes) in &notes {
        let rel = path.strip_prefix(root).unwrap_or(path);
        let bytes = calendar::fmt_bytes(*bytes);
        let _ = writeln!(out, "  {bytes:>9}  {}", rel.display());
    }
    out
}

/// Render one month: the heatmap in `layout`, then the byte Stats block.
fn render_month(
    days: &Days,
    y: i32,
    m: u32,
    today: NaiveDate,
    layout: Layout,
    heat: &Heat,
) -> String {
    let sizes = calendar::month_values(days, y, m);
    let cutoff = (y == today.year() && m == today.month()).then_some(today.day());
    let st = calendar::month_stats(&sizes, cutoff);
    let bytes = |b| calendar::fmt_bytes(b);
    let mut out = calview::month(y, m, &sizes, layout, heat, &bytes);
    push_month_stats(&mut out, &st, y, m);
    out
}

/// Append the `── Stats ──` block for one month (shared shape with the script).
fn push_month_stats(out: &mut String, st: &MonthStats, y: i32, m: u32) {
    out.push_str("\n  ── Stats ─────────────────────────\n");
    let _ = writeln!(
        out,
        "  Days written : {} / {}   ({}%)",
        st.days_written, st.last_day, st.pct
    );
    let _ = writeln!(out, "  Total        : {}", calendar::fmt_bytes(st.total));
    if st.days_written > 0 {
        let avg = st.total / st.days_written as u64;
        let _ = writeln!(out, "  Avg / day    : {}", calendar::fmt_bytes(avg));
    } else {
        out.push_str("  Avg / day    : —\n");
    }
    let _ = writeln!(out, "  Longest run  : {} days", st.longest_run);
    let _ = writeln!(out, "  Current run  : {} days", st.current_run);
    if st.best_day > 0 {
        let mon = NaiveDate::from_ymd_opt(y, m, st.best_day)
            .expect("best_day is in-month")
            .format("%b");
        let _ = writeln!(
            out,
            "  Best day     : {} {}   ({})",
            mon,
            st.best_day,
            calendar::fmt_bytes(st.best_size)
        );
    }
    // The script's trailing `printf "\n"` is supplied by `main`'s `println!`.
}

/// Render a whole year: the chosen heatmap layout followed by the year Stats
/// block. Ports the `year` arm of the script's dispatch.
fn render_year(
    sizes: &[u64],
    y: i32,
    layout: Layout,
    plot: bool,
    today: NaiveDate,
    heat: &Heat,
) -> String {
    let bytes = |b| calendar::fmt_bytes(b);
    let mut out = if plot {
        calview::plot_year(y, sizes, "bytes", &bytes)
    } else {
        calview::year(y, sizes, today, layout, heat, &bytes)
    };
    let cutoff = (y == today.year()).then_some(today.ordinal());
    let st = calendar::year_stats(sizes, y, cutoff);
    push_year_stats(&mut out, &st, y);
    out
}

/// Append the `── Year stats ──` block (shared shape with the script).
fn push_year_stats(out: &mut String, st: &YearStats, y: i32) {
    out.push_str("\n  ── Year stats ───────────────────────────\n");
    let _ = writeln!(
        out,
        "  Days written : {} / {}   ({}%)",
        st.days_written, st.total_days, st.pct
    );
    let _ = writeln!(out, "  Total        : {}", calendar::fmt_bytes(st.total));
    if st.days_written > 0 {
        let avg = st.total / st.days_written as u64;
        let _ = writeln!(out, "  Avg / day    : {}", calendar::fmt_bytes(avg));
    }
    let _ = writeln!(out, "  Longest run  : {} days", st.longest_run);
    let _ = writeln!(out, "  Current run  : {} days", st.current_run);
    if st.best_month > 0 {
        let name = NaiveDate::from_ymd_opt(y, st.best_month, 1)
            .unwrap()
            .format("%B")
            .to_string();
        let _ = writeln!(
            out,
            "  Best month   : {name:<9} ({} days, {})",
            st.best_month_days,
            calendar::fmt_bytes(st.best_month_total)
        );
    }
    if st.best_day_month > 0 {
        let mon = NaiveDate::from_ymd_opt(y, st.best_day_month, st.best_day_dom)
            .unwrap()
            .format("%b");
        let _ = writeln!(
            out,
            "  Best day     : {} {}   ({})",
            mon,
            st.best_day_dom,
            calendar::fmt_bytes(st.best_size)
        );
    }
    // Trailing `printf "\n"` supplied by `main`'s `println!`.
}

/// Plot layout for a month: the shared line chart of daily bytes, then the
/// month Stats block.
fn render_plot_month(days: &Days, y: i32, m: u32, today: NaiveDate) -> String {
    let sizes = calendar::month_values(days, y, m);
    let mut out = calview::plot_month(y, m, &sizes, "bytes", &|b| calendar::fmt_bytes(b));
    let cutoff = (y == today.year() && m == today.month()).then_some(today.day());
    let st = calendar::month_stats(&sizes, cutoff);
    push_month_stats(&mut out, &st, y, m);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 19).unwrap()
    }

    fn none() -> Option<String> {
        None
    }

    #[test]
    fn day_is_yy_mm_dd() {
        let day = |f: [&str; 3]| parse_day(&f.map(String::from));
        let want = NaiveDate::from_ymd_opt(2025, 9, 27).unwrap();
        assert_eq!(day(["25", "09", "27"]), Ok(want));
        assert_eq!(day(["2025", "9", "27"]), Ok(want));
        let err = day(["26", "02", "30"]).unwrap_err();
        assert_eq!(err, "plc stat: invalid day: 26 02 30");
        assert!(day(["26", "13", "01"]).is_err());
    }

    #[test]
    fn defaults_to_current_month() {
        let (y, m) = resolve(none(), none(), &[], today()).unwrap();
        assert_eq!((y, m), (2026, 7));
    }

    #[test]
    fn single_positional_is_month() {
        let (y, m) = resolve(none(), none(), &["4".into()], today()).unwrap();
        assert_eq!((y, m), (2026, 4));
    }

    #[test]
    fn two_positionals_month_year() {
        let (y, m) = resolve(none(), none(), &["5".into(), "24".into()], today()).unwrap();
        assert_eq!((y, m), (2024, 5));
    }

    #[test]
    fn three_positionals_discard_day() {
        let pos = ["10".into(), "10".into(), "24".into()];
        let (y, m) = resolve(none(), none(), &pos, today()).unwrap();
        assert_eq!((y, m), (2024, 10));
    }

    #[test]
    fn flags_month_and_year() {
        let (y, m) = resolve(Some("12".into()), Some("2025".into()), &[], today()).unwrap();
        assert_eq!((y, m), (2025, 12));
    }

    #[test]
    fn positional_overrides_flag_month() {
        let (y, m) = resolve(Some("12".into()), none(), &["3".into()], today()).unwrap();
        assert_eq!((y, m), (2026, 3));
    }

    #[test]
    fn invalid_month_rejected() {
        assert!(resolve(none(), none(), &["13".into()], today()).is_err());
        assert!(resolve(Some("0".into()), none(), &[], today()).is_err());
    }

    #[test]
    fn too_many_positionals_rejected() {
        let pos = ["1".into(), "2".into(), "3".into(), "4".into()];
        assert!(resolve(none(), none(), &pos, today()).is_err());
    }
}
