//! Weeks that start on any weekday, each named after the ISO 8601 week
//! that holds most of its days: the ISO week of its one Thursday. From
//! Monday, that is exactly the ISO week.

use chrono::{Datelike, Days, NaiveDate, Weekday};

/// The first day of the week holding `date`.
pub fn first_day(date: NaiveDate, start: Weekday) -> NaiveDate {
    date - Days::new(date.weekday().days_since(start).into())
}

/// The week holding `date`, as `YYYY-Www`.
pub fn label(date: NaiveDate, start: Weekday) -> String {
    let to_thursday = Weekday::Thu.days_since(start).into();
    let thursday = first_day(date, start) + Days::new(to_thursday);
    thursday.format("%G-W%V").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use Weekday::*;

    const STARTS: [Weekday; 7] = [Mon, Tue, Wed, Thu, Fri, Sat, Sun];

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn days() -> impl Iterator<Item = NaiveDate> {
        date(2019, 12, 1).iter_days().take_while(|d| d.year() < 2031)
    }

    fn iso(d: NaiveDate) -> String {
        d.format("%G-W%V").to_string()
    }

    #[test]
    fn monday_is_iso() {
        for d in days() {
            assert_eq!(label(d, Mon), iso(d), "{d}");
        }
    }

    #[test]
    fn sunday_moves_only_sundays() {
        for d in days() {
            let want = match d.weekday() {
                Sun => iso(d + Days::new(1)),
                _ => iso(d),
            };
            assert_eq!(label(d, Sun), want, "{d}");
        }
        assert_eq!(label(date(2027, 1, 2), Sun), "2026-W53");
        assert_eq!(label(date(2027, 1, 3), Sun), "2027-W01");
    }

    #[test]
    fn a_week_is_named_after_most_of_its_days() {
        for start in STARTS {
            for d in days().filter(|d| d.weekday() == start) {
                let l = label(d, start);
                let week = d.iter_days().take(7);
                let agree = week.filter(|&x| iso(x) == l).count();
                assert!(agree >= 4, "{start} {d}: {l}");
            }
        }
    }

    #[test]
    fn a_label_turns_over_exactly_on_the_start_day() {
        for start in STARTS {
            for (prev, d) in days().zip(days().skip(1)) {
                let turned = label(d, start) != label(prev, start);
                assert_eq!(turned, d.weekday() == start, "{start} {d}");
            }
        }
    }

    #[test]
    fn weeks_count_up_from_one_each_year() {
        for start in STARTS {
            let mut prev: Option<(i32, u32)> = None;
            for d in days().filter(|d| d.weekday() == start) {
                let l = label(d, start);
                let y: i32 = l[..4].parse().unwrap();
                let w: u32 = l[6..].parse().unwrap();
                if let Some((py, pw)) = prev {
                    let next = if y == py { pw + 1 } else { 1 };
                    assert!(y == py || y == py + 1, "{start} {d}");
                    assert_eq!(w, next, "{start} {d}");
                }
                assert!((1..=53).contains(&w), "{start} {d}");
                prev = Some((y, w));
            }
        }
    }

    #[test]
    fn first_day_is_on_or_before() {
        let d = date(2026, 11, 13);
        assert_eq!(first_day(d, Mon), date(2026, 11, 9));
        assert_eq!(first_day(d, Sun), date(2026, 11, 8));
        assert_eq!(first_day(d, Fri), d);
        assert_eq!(first_day(d, Sat), date(2026, 11, 7));
    }
}
