//! Daily-note activity stats — the data behind `plc stat` (ports `_calendar.sh`).
//!
//! Every daily note is scored by its on-disk **byte size** (never its content);
//! sizes drive a heatmap glyph and a set of streak/total statistics. The math
//! here is pure given a slice of sizes, so month/year aggregation is unit-tested
//! without touching the filesystem. Date/weekday layout lives in the `plc stat`
//! command (which already depends on chrono); this crate stays chrono-free.

use std::fs;
use std::path::{Path, PathBuf};

/// Path to the daily note for `y-m-d` under a vault `root`:
/// `notes/management/daily/<YYYY>/<MM>/<YYYY>-<MM>-<DD>.md` — the same string
/// the `daily` and `fin` commands build.
pub fn day_path(root: &Path, y: i32, m: u32, d: u32) -> PathBuf {
    root.join(format!(
        "notes/management/daily/{y:04}/{m:02}/{y:04}-{m:02}-{d:02}.md"
    ))
}

/// On-disk byte size of `path`, or 0 when it is missing/unreadable (a missing
/// daily note counts as an empty day). Ports `stat -f%z … || echo 0`.
pub fn size_of(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Heatmap glyph for a byte size, by the same buckets as the script's `sym()`:
/// `·` empty · `░` <1KB · `▒` 1–4KB · `▓` 4–10KB · `█` >10KB.
pub fn symbol(bytes: u64) -> char {
    match bytes {
        0 => '·',
        b if b < 1024 => '░',
        b if b < 4096 => '▒',
        b if b < 10240 => '▓',
        _ => '█',
    }
}

/// Heatmap glyph for a day's money amount (minor units), by fixed buckets:
/// `·` empty · `░` <5 · `▒` <20 · `▓` <50 · `█` ≥50 (major currency units).
pub fn money_symbol(minor: u64) -> char {
    match minor {
        0 => '·',
        v if v < 500 => '░',
        v if v < 2000 => '▒',
        v if v < 5000 => '▓',
        _ => '█',
    }
}

/// A money amount (minor units) as `<whole>.<cc> <CUR>`, e.g. `12.50 EUR`.
pub fn fmt_money(minor: u64, currency: &str) -> String {
    format!("{}.{:02} {currency}", minor / 100, minor % 100)
}

/// Human byte size: `B`, `KB` (1 dp), or `MB` (2 dp). Ports the script's `fmt()`.
pub fn fmt_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1_048_576 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / 1_048_576.0)
    }
}

/// Days in month `m` (1–12) of year `y`, honoring the Gregorian leap rule.
/// Replaces the script's `cal … | awk` last-day probe.
pub fn last_day_of_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Aggregate statistics over one month's daily sizes; `sizes[i]` is day `i + 1`.
pub struct MonthStats {
    pub days_written: u32,
    pub last_day: u32,
    pub total: u64,
    pub longest_run: u32,
    pub current_run: u32,
    /// 1-based day of the largest note, or 0 when the month is empty.
    pub best_day: u32,
    pub best_size: u64,
    pub pct: u32,
}

/// Compute month stats from per-day sizes. `cutoff` is the day to count the
/// current run back from — pass the current day-of-month when rendering *this*
/// month (so a still-empty today doesn't mask an ongoing streak), else `None`
/// to count back from the last day. Ties on the best day resolve to the first.
pub fn month_stats(sizes: &[u64], cutoff: Option<u32>) -> MonthStats {
    let last_day = sizes.len() as u32;
    let mut days_written = 0;
    let mut total = 0;
    let mut longest_run = 0;
    let mut run = 0;
    let mut best_day = 0;
    let mut best_size = 0;
    for (i, &s) in sizes.iter().enumerate() {
        if s > 0 {
            days_written += 1;
            total += s;
            run += 1;
            if run > longest_run {
                longest_run = run;
            }
            if s > best_size {
                best_size = s;
                best_day = i as u32 + 1;
            }
        } else {
            run = 0;
        }
    }

    let end = cutoff.unwrap_or(last_day);
    let mut current_run = 0;
    let mut d = end;
    while d >= 1 && sizes.get((d - 1) as usize).copied().unwrap_or(0) > 0 {
        current_run += 1;
        d -= 1;
    }

    let pct = if last_day > 0 {
        days_written * 100 / last_day
    } else {
        0
    };

    MonthStats {
        days_written,
        last_day,
        total,
        longest_run,
        current_run,
        best_day,
        best_size,
        pct,
    }
}

/// Per-day byte sizes for a whole year, indexed by day-of-year − 1 (Jan 1 = 0),
/// walking Jan→Dec. Length is the number of days in `y`. Ports `collect_year`.
pub fn collect_year(root: &Path, y: i32) -> Vec<u64> {
    let mut sizes = Vec::with_capacity(366);
    for m in 1..=12 {
        for d in 1..=last_day_of_month(y, m) {
            sizes.push(size_of(&day_path(root, y, m, d)));
        }
    }
    sizes
}

/// Aggregate statistics over one year's day-of-year sizes (`collect_year`).
pub struct YearStats {
    pub days_written: u32,
    pub total_days: u32,
    pub total: u64,
    pub longest_run: u32,
    pub current_run: u32,
    /// Month (1-based) with the largest byte total, or 0 when the year is empty.
    pub best_month: u32,
    pub best_month_days: u32,
    pub best_month_total: u64,
    /// Month + day-of-month of the single largest note (0/0 when empty).
    pub best_day_month: u32,
    pub best_day_dom: u32,
    pub best_size: u64,
    pub pct: u32,
}

/// Compute year stats from day-of-year sizes. `year` supplies month lengths (so
/// day-of-year maps back to month/day); `cutoff_doy` is the day-of-year to count
/// the current run back from — pass today's ordinal for the current year, else
/// `None`. Ports `render_year_stats`; ties on best month/day resolve to first.
pub fn year_stats(sizes: &[u64], year: i32, cutoff_doy: Option<u32>) -> YearStats {
    let mut days_written = 0;
    let mut total = 0;
    let mut longest_run = 0;
    let mut run = 0;
    let mut best_size = 0;
    let mut best_day_month = 0;
    let mut best_day_dom = 0;
    let mut best_month = 0;
    let mut best_month_total = 0;
    let mut best_month_days = 0;

    let mut doy = 0usize;
    for m in 1..=12 {
        let last = last_day_of_month(year, m);
        let mut month_total = 0;
        let mut month_days = 0;
        for d in 1..=last {
            let sz = sizes.get(doy).copied().unwrap_or(0);
            doy += 1;
            if sz > 0 {
                days_written += 1;
                total += sz;
                run += 1;
                if run > longest_run {
                    longest_run = run;
                }
                if sz > best_size {
                    best_size = sz;
                    best_day_month = m;
                    best_day_dom = d;
                }
                month_days += 1;
                month_total += sz;
            } else {
                run = 0;
            }
        }
        if month_total > best_month_total {
            best_month_total = month_total;
            best_month_days = month_days;
            best_month = m;
        }
    }

    let total_days = doy as u32;
    let end = cutoff_doy.unwrap_or(total_days);
    let mut current_run = 0;
    let mut dd = end;
    while dd >= 1 && sizes.get((dd - 1) as usize).copied().unwrap_or(0) > 0 {
        current_run += 1;
        dd -= 1;
    }

    let pct = if total_days > 0 {
        days_written * 100 / total_days
    } else {
        0
    };

    YearStats {
        days_written,
        total_days,
        total,
        longest_run,
        current_run,
        best_month,
        best_month_days,
        best_month_total,
        best_day_month,
        best_day_dom,
        best_size,
        pct,
    }
}

/// Render an ASCII line chart of `values` (one column each) as `height + 1`
/// lines: `height` plotted rows with right-aligned byte-axis labels, then the
/// bottom axis. Uses box-drawing connectors (`● ─ │ ╭ ╮ ╰ ╯`). Ports the
/// script's `draw_line_chart`; `max` is the top of the y-axis (clamped to ≥1).
pub fn line_chart(max: u64, width: usize, height: usize, values: &[u64]) -> Vec<String> {
    line_chart_with(max, width, height, values, &|b| fmt_bytes(b))
}

/// Like [`line_chart`], but the y-axis labels are rendered by `fmt` instead of
/// [`fmt_bytes`] — so the same chart can label a byte axis or a money axis.
pub fn line_chart_with(
    max: u64,
    width: usize,
    height: usize,
    values: &[u64],
    fmt: &dyn Fn(u64) -> String,
) -> Vec<String> {
    if width == 0 || height < 2 {
        return Vec::new();
    }
    let max = max.max(1);
    let span = height as u64 - 1;

    // Row (0 = top) each column's point lands on.
    let rows: Vec<usize> = (0..width)
        .map(|i| {
            let v = values.get(i).copied().unwrap_or(0).min(max);
            (((max - v) * span) / max) as usize
        })
        .collect();

    let mut grid = vec![vec![' '; width]; height];
    grid[rows[0]][0] = '●';
    for i in 1..width {
        let (r, prev) = (rows[i], rows[i - 1]);
        if prev == r {
            grid[r][i] = '─';
        } else if prev < r {
            grid[prev][i] = '╮';
            for cell in grid.iter_mut().take(r).skip(prev + 1) {
                cell[i] = '│';
            }
            grid[r][i] = '╰';
        } else {
            grid[prev][i] = '╯';
            for cell in grid.iter_mut().take(prev).skip(r + 1) {
                cell[i] = '│';
            }
            grid[r][i] = '╭';
        }
    }

    let mut lines = Vec::with_capacity(height + 1);
    for (r, gridrow) in grid.iter().enumerate() {
        let val = max * (span - r as u64) / span;
        let sep = if r == 0 { '┼' } else { '┤' };
        let mut line = format!("{:>9} {sep}", fmt(val));
        line.extend(gridrow.iter());
        lines.push(line);
    }
    let mut bottom = format!("{:>9} └", "0");
    bottom.extend(std::iter::repeat_n('─', width));
    lines.push(bottom);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_buckets() {
        assert_eq!(symbol(0), '·');
        assert_eq!(symbol(1), '░');
        assert_eq!(symbol(1023), '░');
        assert_eq!(symbol(1024), '▒');
        assert_eq!(symbol(4095), '▒');
        assert_eq!(symbol(4096), '▓');
        assert_eq!(symbol(10239), '▓');
        assert_eq!(symbol(10240), '█');
    }

    #[test]
    fn money_symbol_buckets() {
        assert_eq!(money_symbol(0), '·');
        assert_eq!(money_symbol(499), '░'); // <5.00
        assert_eq!(money_symbol(500), '▒'); // 5.00
        assert_eq!(money_symbol(1999), '▒');
        assert_eq!(money_symbol(2000), '▓'); // 20.00
        assert_eq!(money_symbol(4999), '▓');
        assert_eq!(money_symbol(5000), '█'); // 50.00
        assert_eq!(fmt_money(1250, "EUR"), "12.50 EUR");
    }

    #[test]
    fn fmt_bytes_units() {
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1024), "1.0 KB");
        assert_eq!(fmt_bytes(1536), "1.5 KB");
        assert_eq!(fmt_bytes(1_048_576), "1.00 MB");
        assert_eq!(fmt_bytes(1_572_864), "1.50 MB");
    }

    #[test]
    fn last_day_leap_and_common() {
        assert_eq!(last_day_of_month(2024, 2), 29);
        assert_eq!(last_day_of_month(2026, 2), 28);
        assert_eq!(last_day_of_month(2000, 2), 29);
        assert_eq!(last_day_of_month(1900, 2), 28);
        assert_eq!(last_day_of_month(2026, 4), 30);
        assert_eq!(last_day_of_month(2026, 12), 31);
    }

    #[test]
    fn empty_month() {
        let st = month_stats(&[0, 0, 0], None);
        assert_eq!(st.days_written, 0);
        assert_eq!(st.total, 0);
        assert_eq!(st.longest_run, 0);
        assert_eq!(st.current_run, 0);
        assert_eq!(st.best_day, 0);
        assert_eq!(st.pct, 0);
    }

    #[test]
    fn full_month_all_written() {
        let sizes = [100u64; 30];
        let st = month_stats(&sizes, None);
        assert_eq!(st.days_written, 30);
        assert_eq!(st.total, 3000);
        assert_eq!(st.longest_run, 30);
        assert_eq!(st.current_run, 30);
        assert_eq!(st.pct, 100);
    }

    #[test]
    fn gap_breaks_runs() {
        // days: 1,2 written, 3 empty, 4,5,6 written (last day)
        let st = month_stats(&[10, 20, 0, 30, 40, 50], None);
        assert_eq!(st.days_written, 5);
        assert_eq!(st.longest_run, 3);
        // current run counts back from the last day: 4,5,6 → 3
        assert_eq!(st.current_run, 3);
        assert_eq!(st.best_size, 50);
        assert_eq!(st.best_day, 6);
    }

    #[test]
    fn cutoff_counts_current_run_from_today() {
        // Current month: today is day 3 (empty), days 1,2 written. Counting from
        // day 3 back hits the empty today first → current run 0, but the streak
        // before it is preserved in longest_run.
        let st = month_stats(&[10, 20, 0, 0, 0], Some(3));
        assert_eq!(st.current_run, 0);
        assert_eq!(st.longest_run, 2);
    }

    #[test]
    fn best_day_tie_takes_first() {
        let st = month_stats(&[50, 50, 10], None);
        assert_eq!(st.best_day, 1);
        assert_eq!(st.best_size, 50);
    }

    #[test]
    fn year_stats_leap_length_and_best_picks() {
        // 2024 is a leap year → 366 days. Build a year where Feb 29 is the single
        // largest note and March carries the largest monthly total.
        let mut sizes = vec![0u64; 366];
        sizes[59] = 9000; // day-of-year 60 = Feb 29, 2024 (idx 59) — biggest single day
        for i in 60..91 {
            sizes[i] = 400; // all of March (31 days) → 12400, the biggest month
        }
        let st = year_stats(&sizes, 2024, None);
        assert_eq!(st.total_days, 366);
        assert_eq!(st.best_day_month, 2);
        assert_eq!(st.best_day_dom, 29);
        assert_eq!(st.best_size, 9000);
        assert_eq!(st.best_month, 3);
        assert_eq!(st.best_month_days, 31);
        assert_eq!(st.best_month_total, 12400);
        // Feb 29 (idx 59) then March (idx 60..90) are contiguous → run of 32.
        assert_eq!(st.longest_run, 32);
    }

    #[test]
    fn line_chart_shape_and_connectors() {
        // A V: high, low, high over 3 columns, y-axis height 3.
        let lines = line_chart(100, 3, 3, &[0, 100, 0]);
        assert_eq!(lines.len(), 4); // 3 plotted rows + bottom axis
        // Top row dips in the middle: space, then a peak joining down on both sides.
        assert!(lines[0].ends_with(" ╭╮"), "{}", lines[0]);
        // Bottom plotted row: start marker ● then the two valley connectors.
        assert!(lines[2].ends_with("●╯╰"), "{}", lines[2]);
        // Axis labels: top = max, bottom plotted row = 0.
        assert!(lines[0].contains("100 B"), "{}", lines[0]);
        assert!(lines[2].trim_start().starts_with("0 B"), "{}", lines[2]);
        assert!(lines[3].ends_with("───"), "{}", lines[3]);
    }

    #[test]
    fn line_chart_degenerate_inputs() {
        assert!(line_chart(100, 0, 8, &[]).is_empty());
        assert!(line_chart(0, 3, 1, &[1, 2, 3]).is_empty());
    }

    #[test]
    fn year_stats_current_run_from_cutoff() {
        let mut sizes = vec![0u64; 365];
        sizes[0] = 10; // Jan 1
        sizes[1] = 20; // Jan 2
        // cutoff at day-of-year 3 (empty) → current run 0, prior streak preserved.
        let st = year_stats(&sizes, 2026, Some(3));
        assert_eq!(st.current_run, 0);
        assert_eq!(st.longest_run, 2);
    }
}
