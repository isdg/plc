//! `plc stat` — the bytes written each day, as an ASCII calendar + stats.
//!
//! Ports `_calendar.sh` (the `pst` script): scores each day by the bytes
//! written that day across the vault (see [`activity`]). A date (see
//! [`when`]) names a day, whose notes it lists, or a month or year, which
//! it draws as a heatmap scaled to the year, or a plot, then a stats block
//! (days written, total, avg, streaks, best day).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use chrono::{Datelike, Local, NaiveDate};
use clap::{Args, ValueEnum};
use plc_core::activity;
use plc_core::calendar::{self, MonthStats, YearStats};

use crate::cmd::calview::{self, Cells, ColorArgs, Format, Heat, Layout};
use crate::cmd::when::{self, Scope};
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
    /// `DD [MM [YY]]`, as `plc daily` takes it, today's for any field left
    /// off; `-` for a whole field shows a month (`- MM YY`) or a year
    /// (`- - YY`). None is today; `-` is this month.
    #[arg(value_name = "DD MM YY")]
    date: Vec<String>,
    /// How to show it: a day as `list` (default) or `tsv`; a month or year
    /// as `grid` (default), `rows`, `plot` or `tsv`.
    #[arg(short = 'f', long, value_enum, value_name = "FORMAT")]
    format: Option<Format>,
    #[command(flatten)]
    color: ColorArgs,
}

pub fn run(palace: &Palace, args: StatArgs) -> Result<String, String> {
    let today = Local::now().date_naive();
    let scope = when::parse(&args.date, today)
        .map_err(|e| format!("plc stat: {e}"))?;
    let root = palace.root();
    let format = args.format.unwrap_or(match scope {
        Scope::Day(_) => Format::List,
        _ => Format::Grid,
    });
    let year = match scope {
        Scope::Day(day) => {
            return match format {
                Format::List => Ok(render_day(root, day)),
                Format::Tsv => Ok(day_tsv(root, day)),
                f => Err(format!(
                    "plc stat: -f {} shows a month or a year: - MM YY",
                    name(f)
                )),
            };
        }
        Scope::Month(y, _) | Scope::Year(y) => y,
    };
    if format == Format::List {
        return Err("plc stat: -f list shows a day: DD MM YY".to_string());
    }
    let days = activity::by_day(root);
    let values = calendar::year_values(&days, year);
    let heat = bytes_heat(&values, args.color.cells());
    Ok(match (scope, format) {
        (Scope::Month(y, m), Format::Tsv) => {
            let last = calendar::last_day_of_month(y, m);
            days_tsv(&days, ymd(y, m, 1), ymd(y, m, last))
        }
        (Scope::Month(y, m), Format::Plot) => {
            render_plot_month(&days, y, m, today)
        }
        (Scope::Month(y, m), f) => {
            render_month(&days, y, m, today, layout(f), &heat)
        }
        (_, Format::Tsv) => days_tsv(&days, ymd(year, 1, 1), ymd(year, 12, 31)),
        (_, f) => {
            let plot = f == Format::Plot;
            render_year(&values, year, layout(f), plot, today, &heat)
        }
    })
}

fn name(format: Format) -> String {
    format
        .to_possible_value()
        .map_or_else(String::new, |v| v.get_name().to_string())
}

fn layout(format: Format) -> Layout {
    match format {
        Format::Rows => Layout::Tab,
        _ => Layout::Git,
    }
}

fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("a day of the scope")
}

/// `YYYY-MM-DD<TAB>bytes` for each day from `first` to `last`.
fn days_tsv(days: &Days, first: NaiveDate, last: NaiveDate) -> String {
    let span = first.iter_days().take_while(|d| *d <= last);
    let lines: Vec<String> = span
        .map(|d| format!("{d}\t{}", days.get(&d).copied().unwrap_or(0)))
        .collect();
    lines.join("\n")
}

/// `path<TAB>bytes` for each note written on `day`, most bytes first.
fn day_tsv(root: &Path, day: NaiveDate) -> String {
    let lines: Vec<String> = activity::on_day(root, day)
        .iter()
        .map(|(path, bytes)| {
            let rel = path.strip_prefix(root).unwrap_or(path);
            format!("{}\t{bytes}", rel.display())
        })
        .collect();
    lines.join("\n")
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
