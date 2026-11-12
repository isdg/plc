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
use clap::{Args, ColorChoice};
use plc_core::calendar;

const SHADES: [char; 5] = ['·', '░', '▒', '▓', '█'];
/// GitHub's contribution ramp, grey to bright green, in xterm-256 colours.
const GREENS: [u8; 5] = [237, 22, 28, 34, 40];

/// How a heatmap draws a day: a shade glyph per level, or the same square
/// in a colour per level, the way GitHub draws contributions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cells {
    Shade,
    Color,
}

#[derive(Args)]
pub struct ColorArgs {
    /// Draw heatmap days as coloured squares: `auto` (when stdout is a
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
                let _ = write!(out, "\x1b[38;5;{}m■\x1b[39m", GREENS[l]);
            }
        }
    }

    /// What follows a day in the year layouts: a square has side bearings of
    /// its own, while shade glyphs would run together without a space.
    fn gap(&self) -> &'static str {
        match self.cells {
            Cells::Shade => " ",
            Cells::Color => "",
        }
    }

    fn cols(&self) -> u32 {
        1 + self.gap().len() as u32
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

/// One month as the title, a Mo–Su grid with a heatmap row under each week, and
/// the legend. `values` holds every day of the month (`len == last day`).
pub fn month_grid(y: i32, m: u32, values: &[u64], heat: &Heat) -> String {
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
                heat.push_cell(&mut out, values[(d - 1) as usize]);
            }
        }
        out.push('\n');
    }
    let _ = writeln!(out, "\n     Legend:  {}", heat.legend());
    out
}

/// GitHub-style 7×weeks contribution grid for a year, with month labels above.
/// `values` is indexed by day-of-year − 1.
pub fn year_git(y: i32, values: &[u64], heat: &Heat) -> String {
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
    let cols = heat.cols();
    let indent = 39 * cols as usize / 2;
    let _ = writeln!(out, "{}{y} activity", " ".repeat(indent));

    out.push_str("      ");
    let mut printed = 0u32;
    for m in 1..=12 {
        let target = month_col[m as usize] * cols;
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
                out.push_str(&" ".repeat(cols as usize));
            } else {
                heat.push_cell(&mut out, values[(d_idx - 1) as usize]);
                out.push_str(heat.gap());
            }
        }
        out.push('\n');
    }
    let _ = writeln!(out, "\n  Legend:  {}", heat.legend());
    out
}

/// One row per month: a heatmap strip and a right-hand `days/total` summary
/// (formatted by `fmt`); the current month is bold. `values` by day-of-year − 1.
pub fn year_tab(
    y: i32,
    values: &[u64],
    today: NaiveDate,
    heat: &Heat,
    fmt: &dyn Fn(u64) -> String,
) -> String {
    let mut out = String::new();
    out.push('\n');
    let cols = heat.cols() as usize;
    let _ = writeln!(out, "{}{y} activity", " ".repeat(30 * cols / 2));
    out.push('\n');

    let mut off = 0usize;
    for m in 1..=12 {
        let last = calendar::last_day_of_month(y, m);
        let label = NaiveDate::from_ymd_opt(y, m, 1).unwrap().format("%b");
        let now = y == today.year() && m == today.month();
        let (b, r) = if now && heat.cells == Cells::Color {
            ("\x1b[1m", "\x1b[0m")
        } else {
            ("", "")
        };
        let mut strip = String::new();
        let mut mtotal = 0u64;
        let mut mdays = 0u32;
        for d in 1..=last {
            let v = values[off + (d - 1) as usize];
            heat.push_cell(&mut strip, v);
            strip.push_str(heat.gap());
            if v > 0 {
                mtotal += v;
                mdays += 1;
            }
        }
        let pad = " ".repeat((31 - last as usize) * cols);
        let _ = writeln!(out, "{b}  {label}  {strip}{pad}   {mdays:2}/{last}   {}{r}", fmt(mtotal));
        off += last as usize;
    }
    let _ = writeln!(out, "\n  Legend:  {}", heat.legend());
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

#[cfg(test)]
mod tests {
    use super::*;

    const HEAT: Heat = Heat {
        level: calendar::level,
        labels: ["0", "1", "2", "3", "4"],
        cells: Cells::Shade,
    };
    const GREY: &str = "\x1b[38;5;237m■\x1b[39m";
    const GREEN: &str = "\x1b[38;5;40m■\x1b[39m";

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
    fn color_cells_are_squares_from_grey_to_green() {
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
        for (c, glyphs) in [(Cells::Shade, "·░▒▓█"), (Cells::Color, "■")] {
            let heat = HEAT.with_cells(c);
            let git = year_git(2025, &values, &heat);
            let tab = year_tab(2025, &values, day(2025, 3, 9), &heat, &fmt);
            for out in [git, tab] {
                let n = out.chars().filter(|ch| glyphs.contains(*ch));
                assert_eq!(n.count(), 365 + 5, "{c:?}");
            }
        }
    }

    /// Display columns of `line`, its escapes dropped.
    fn width(line: &str) -> usize {
        let mut n = 0;
        let mut esc = false;
        for ch in line.chars() {
            match (esc, ch) {
                (false, '\x1b') => esc = true,
                (true, 'm') => esc = false,
                (false, _) => n += 1,
                _ => {}
            }
        }
        n
    }

    #[test]
    fn git_rows_take_one_column_a_week_in_colour() {
        let values = vec![0u64; 365];
        for (c, cols) in [(Cells::Shade, 2), (Cells::Color, 1)] {
            let out = year_git(2025, &values, &HEAT.with_cells(c));
            let row = out.lines().find(|l| l.starts_with("  Mo")).unwrap();
            assert_eq!(width(row), 6 + 53 * cols, "{c:?}");
        }
    }

    #[test]
    fn tab_summaries_line_up_in_both_cells() {
        let values = vec![0u64; 365];
        let fmt = |v: u64| v.to_string();
        let today = day(2025, 3, 9);
        for c in [Cells::Shade, Cells::Color] {
            let out = year_tab(2025, &values, today, &HEAT.with_cells(c), &fmt);
            let at: Vec<usize> = out
                .lines()
                .filter(|l| l.contains('/'))
                .map(|l| width(&l[..l.find('/').unwrap()]))
                .collect();
            assert_eq!(at.len(), 12, "{c:?}");
            assert!(at.iter().all(|&a| a == at[0]), "{c:?}: {at:?}");
        }
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
