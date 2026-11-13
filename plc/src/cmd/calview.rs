//! Shared calendar-view renderers for `plc stat` and `plc ledger stat`.
//!
//! These lay out per-day values (`values[i]` = day `i + 1` for a month, or
//! day-of-year for a year) as an ASCII heatmap or line chart. They are
//! *value-agnostic*: a [`Heat`] buckets each value into a level and names the
//! levels in the legend, and any numeric axis/summary label comes from a
//! `fmt` closure — so `plc stat` passes byte formatters and `plc ledger stat`
//! passes money formatters, sharing one layout. Callers build the `values`
//! (from file sizes, or from ledger spend) and append their own stats block.

use std::fmt::Write as _;
use std::io;

use anstream::AutoStream;
use chrono::{Datelike, NaiveDate};
use clap::{Args, ColorChoice, ValueEnum};
use plc_core::calendar;

const SHADES: [char; 5] = ['·', '░', '▒', '▓', '█'];
/// Seven eighths of the line high, so stacked rows keep a gap between them.
const BLOCK: char = '▇';
/// GitHub's contribution ramp, grey to bright green, in xterm-256 colours.
const GREENS: [u8; 5] = [237, 22, 28, 34, 40];

/// How a heatmap draws a day: a shade glyph per level, or the same block
/// in a colour per level, the way GitHub draws contributions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cells {
    Shade,
    Color,
}

#[derive(Args)]
pub struct ColorArgs {
    /// Draw heatmap days as coloured blocks: `auto` (when stdout is a
    /// terminal), `always`, or `never` (shade glyphs instead).
    #[arg(long, value_name = "WHEN", default_value_t = ColorChoice::Auto)]
    color: ColorChoice,
}

impl ColorArgs {
    /// `auto` defers to anstream: a terminal, unless NO_COLOR or CLICOLOR=0
    /// says otherwise, or CLICOLOR_FORCE forces colour into a pipe.
    pub fn cells(&self) -> Cells {
        let color = match self.color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                AutoStream::choice(&io::stdout())
                    != anstream::ColorChoice::Never
            }
        };
        if color {
            Cells::Color
        } else {
            Cells::Shade
        }
    }
}

/// A heatmap's scale: `level` buckets a value into 0 (empty) to 4, `labels`
/// names each level in the legend, and `cells` draws the levels.
#[derive(Clone, Copy)]
pub struct Heat {
    pub level: fn(u64) -> u8,
    pub labels: [&'static str; 5],
    pub cells: Cells,
}

impl Heat {
    pub fn with_cells(self, cells: Cells) -> Heat {
        Heat { cells, ..self }
    }

    fn push_cell(&self, out: &mut String, v: u64) {
        self.push_level(out, (self.level)(v));
    }

    fn push_level(&self, out: &mut String, level: u8) {
        let l = usize::from(level.min(4));
        match self.cells {
            Cells::Shade => out.push(SHADES[l]),
            // 39 resets the foreground alone, so a bold row stays bold.
            Cells::Color => {
                let _ = write!(out, "\x1b[38;5;{}m{BLOCK}\x1b[39m", GREENS[l]);
            }
        }
    }

    fn legend(&self) -> String {
        let mut out = String::new();
        for (l, label) in (0u8..).zip(self.labels) {
            if l > 0 {
                out.push_str("   ");
            }
            self.push_level(&mut out, l);
            let _ = write!(out, " {label}");
        }
        out
    }
}

/// The heatmap shape for both scopes, as `--layout` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Layout {
    /// A row per weekday and a column per week, as GitHub draws a year.
    Git,
    /// A row of days per month, with a days/total summary.
    Tab,
}

/// Sunday first, as GitHub draws it, so the weekend frames the week.
const DOW: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

/// One month in `layout`. `values` holds every day of the month.
pub fn month(
    y: i32,
    m: u32,
    values: &[u64],
    layout: Layout,
    heat: &Heat,
    fmt: &dyn Fn(u64) -> String,
) -> String {
    match layout {
        Layout::Git => month_git(y, m, values, heat),
        Layout::Tab => month_tab(y, m, values, heat, fmt),
    }
}

/// A whole year in `layout`. `values` is indexed by day-of-year − 1.
pub fn year(
    y: i32,
    values: &[u64],
    today: NaiveDate,
    layout: Layout,
    heat: &Heat,
    fmt: &dyn Fn(u64) -> String,
) -> String {
    match layout {
        Layout::Git => year_git(y, values, heat),
        Layout::Tab => year_tab(y, values, today, heat, fmt),
    }
}

/// A month the way [`year_git`] draws a year, each week's column headed by
/// its first day in the month, so every date stays readable.
fn month_git(y: i32, m: u32, values: &[u64], heat: &Heat) -> String {
    let first = NaiveDate::from_ymd_opt(y, m, 1).expect("first-of-month");
    let pad = first.weekday().num_days_from_sunday() as usize;
    let weeks = (pad + values.len()).div_ceil(7);

    let mut out = String::new();
    let _ = writeln!(out, "\n  {}", first.format("%B %Y"));
    out.push_str("      ");
    for w in 0..weeks {
        let _ = write!(out, "{:>2} ", (w * 7).saturating_sub(pad) + 1);
    }
    out.push('\n');
    for (row, label) in DOW.iter().enumerate() {
        let _ = write!(out, "  {label}  ");
        for w in 0..weeks {
            match (w * 7 + row).checked_sub(pad).and_then(|i| values.get(i)) {
                Some(&v) => {
                    out.push(' ');
                    heat.push_cell(&mut out, v);
                    out.push(' ');
                }
                None => out.push_str("   "),
            }
        }
        out.push('\n');
    }
    let _ = writeln!(out, "\n  Legend:  {}", heat.legend());
    out
}

/// A month as one [`year_tab`] row, with a day axis under it.
fn month_tab(
    y: i32,
    m: u32,
    values: &[u64],
    heat: &Heat,
    fmt: &dyn Fn(u64) -> String,
) -> String {
    let first = NaiveDate::from_ymd_opt(y, m, 1).expect("first-of-month");
    let mut out = String::new();
    let _ = writeln!(out, "\n  {}\n", first.format("%B %Y"));
    let label = first.format("%b").to_string();
    push_tab_row(&mut out, &label, values, false, heat, fmt);
    let mut axis = " ".repeat(7);
    for d in 1..=values.len() {
        let mark = d == 1 || d % 5 == 0;
        axis.push_str(&if mark { format!("{d:<2}") } else { "  ".into() });
    }
    let _ = writeln!(out, "{}", axis.trim_end());
    let _ = writeln!(out, "\n  Legend:  {}", heat.legend());
    out
}

/// GitHub-style 7×weeks contribution grid for a year, with month labels above.
/// `values` is indexed by day-of-year − 1.
fn year_git(y: i32, values: &[u64], heat: &Heat) -> String {
    let total_days = values.len() as u32;
    let jan1 = NaiveDate::from_ymd_opt(y, 1, 1).unwrap();
    let pad = jan1.weekday().num_days_from_sunday();
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

    for (row, label) in DOW.iter().enumerate() {
        let _ = write!(out, "  {label}  ");
        for c in 0..weeks {
            let d_idx = grid[(c * 7 + row as u32) as usize];
            if d_idx == 0 {
                out.push_str("  ");
            } else {
                heat.push_cell(&mut out, values[(d_idx - 1) as usize]);
                out.push(' ');
            }
        }
        out.push('\n');
    }
    let _ = writeln!(out, "\n  Legend:  {}", heat.legend());
    out
}

/// One row per month: a heatmap strip and a right-hand `days/total` summary
/// (formatted by `fmt`); the current month is bold. `values` by day-of-year − 1.
fn year_tab(
    y: i32,
    values: &[u64],
    today: NaiveDate,
    heat: &Heat,
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
        let now = y == today.year() && m == today.month();
        let days = &values[off..off + last as usize];
        push_tab_row(&mut out, &label.to_string(), days, now, heat, fmt);
        off += last as usize;
    }
    let _ = writeln!(out, "\n  Legend:  {}", heat.legend());
    out
}

/// One tab row: `label`, a strip of `values` padded to 31 days so rows
/// line up, and the days written / total; `bold` only takes with colour.
fn push_tab_row(
    out: &mut String,
    label: &str,
    values: &[u64],
    bold: bool,
    heat: &Heat,
    fmt: &dyn Fn(u64) -> String,
) {
    let (b, r) = if bold && heat.cells == Cells::Color {
        ("\x1b[1m", "\x1b[0m")
    } else {
        ("", "")
    };
    let mut strip = String::new();
    for &v in values {
        heat.push_cell(&mut strip, v);
        strip.push(' ');
    }
    let days = values.iter().filter(|&&v| v > 0).count();
    let total = fmt(values.iter().sum());
    let pad = " ".repeat(2 * (31 - values.len()));
    let last = values.len();
    let _ = writeln!(
        out,
        "{b}  {label}  {strip}{pad}   {days:2}/{last}   {total}{r}"
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    const HEAT: Heat = Heat {
        level: calendar::level,
        labels: ["0", "1", "2", "3", "4"],
        cells: Cells::Shade,
    };
    const GREY: &str = "\x1b[38;5;237m▇\x1b[39m";
    const GREEN: &str = "\x1b[38;5;40m▇\x1b[39m";

    fn cells(heat: &Heat, values: &[u64]) -> String {
        let mut out = String::new();
        for &v in values {
            heat.push_cell(&mut out, v);
        }
        out
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn shade_cells_are_glyphs() {
        assert_eq!(cells(&HEAT, &[0, 2048, 20_000]), "·▒█");
    }

    #[test]
    fn color_cells_are_blocks_from_grey_to_green() {
        let heat = HEAT.with_cells(Cells::Color);
        assert_eq!(cells(&heat, &[0, 20_000]), format!("{GREY}{GREEN}"));
    }

    #[test]
    fn legend_draws_each_level_as_a_cell() {
        assert_eq!(HEAT.legend(), "· 0   ░ 1   ▒ 2   ▓ 3   █ 4");
        let legend = HEAT.with_cells(Cells::Color).legend();
        assert!(legend.starts_with(&format!("{GREY} 0   ")));
        assert!(legend.ends_with(&format!("{GREEN} 4")));
    }

    #[test]
    fn year_layouts_draw_every_day_and_legend_level() {
        let values = vec![0u64; 365];
        let fmt = |v: u64| v.to_string();
        for (c, glyphs) in [(Cells::Shade, "·░▒▓█"), (Cells::Color, "▇")] {
            let heat = HEAT.with_cells(c);
            let git = year_git(2025, &values, &heat);
            let tab = year_tab(2025, &values, day(2025, 3, 9), &heat, &fmt);
            for out in [git, tab] {
                let n = out.chars().filter(|ch| glyphs.contains(*ch));
                assert_eq!(n.count(), 365 + 5, "{c:?}");
            }
        }
    }

    #[test]
    fn month_git_heads_each_week_with_its_first_day() {
        let out = month_git(2026, 11, &[0; 30], &HEAT);
        let head = out.lines().nth(2).unwrap();
        let head: Vec<&str> = head.split_whitespace().collect();
        assert_eq!(head, ["1", "8", "15", "22", "29"]);
        let rows: Vec<&str> = out.lines().skip(3).take(7).collect();
        assert!(rows[0].starts_with("  Su   · "), "{out}");
        assert!(rows[1].starts_with("  Mo   · "), "{out}");
        assert!(rows[6].starts_with("  Sa   · "), "{out}");

        let out = month_git(2026, 12, &[0; 31], &HEAT);
        let rows: Vec<&str> = out.lines().skip(3).take(7).collect();
        assert!(rows[0].starts_with("  Su      · "), "{out}");
        assert!(rows[2].starts_with("  Tu   · "), "{out}");
    }

    #[test]
    fn month_layouts_draw_every_day_once() {
        let fmt = |v: u64| v.to_string();
        for (y, m, last) in [(2026, 2, 28), (2026, 11, 30), (2024, 12, 31)] {
            let values = vec![0u64; last];
            for c in [Cells::Shade, Cells::Color] {
                let heat = HEAT.with_cells(c);
                for layout in [Layout::Git, Layout::Tab] {
                    let out = month(y, m, &values, layout, &heat, &fmt);
                    let n = out
                        .chars()
                        .filter(|ch| SHADES.contains(ch) || *ch == BLOCK);
                    assert_eq!(n.count(), last + 5, "{y}-{m} {layout:?} {c:?}");
                }
            }
        }
    }

    #[test]
    fn month_tab_sums_and_marks_every_fifth_day() {
        let mut values = vec![0u64; 30];
        values[3] = 7;
        values[9] = 5;
        let out = month_tab(2026, 11, &values, &HEAT, &|v| format!("{v}B"));
        let row = out.lines().find(|l| l.starts_with("  Nov  ")).unwrap();
        assert!(row.ends_with(" 2/30   12B"), "{row}");
        let axis: Vec<&str> = out
            .lines()
            .find(|l| l.trim_start().starts_with("1 "))
            .unwrap()
            .split_whitespace()
            .collect();
        assert_eq!(axis, ["1", "5", "10", "15", "20", "25", "30"]);
    }

    #[test]
    fn tab_bolds_the_current_month_only_in_colour() {
        let values = vec![0u64; 365];
        let fmt = |v: u64| v.to_string();
        let today = day(2025, 3, 9);
        let tab = |c| year_tab(2025, &values, today, &HEAT.with_cells(c), &fmt);
        assert!(!tab(Cells::Shade).contains('\x1b'));
        let color = tab(Cells::Color);
        let bold: Vec<_> =
            color.lines().filter(|l| l.contains("\x1b[1m")).collect();
        assert_eq!(bold.len(), 1);
        assert!(bold[0].starts_with("\x1b[1m  Mar  "));
    }
}
