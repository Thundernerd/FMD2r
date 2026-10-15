//! The download schedule: a daily window outside which no downloads run. FMD2 has none; this
//! is FMD2r's own (issue #310).

use std::cmp::Ordering;
use std::time::Duration;

use fmd_http::BoxFuture;
use jiff::civil::Time;

use crate::settings::ConnectionSettings;

/// The server's local time, for the download schedule.
pub trait Clock: Send + Sync + 'static {
    /// The local time of day.
    fn now(&self) -> Time;

    /// Resolves once `duration` has passed on this clock.
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()>;
}

/// The system's clock and time zone.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Time {
        jiff::Zoned::now().time()
    }

    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// A daily window in the server's local time, from `start` up to `end`. A window whose end
/// is before its start crosses midnight; one whose start equals its end is open all day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    start: Time,
    end: Time,
}

impl Schedule {
    pub fn new(start: Time, end: Time) -> Schedule {
        Schedule { start, end }
    }

    /// The schedule the settings set, or `None` when downloads run at any hour.
    pub fn of(connections: &ConnectionSettings) -> Option<Schedule> {
        if !connections.schedule_enabled {
            return None;
        }
        Some(Schedule::new(
            parse_time(&connections.schedule_start)?,
            parse_time(&connections.schedule_end)?,
        ))
    }

    /// When the window opens.
    pub fn start(&self) -> Time {
        self.start
    }

    pub fn is_open(&self, now: Time) -> bool {
        let (start, end) = (self.start, self.end);
        match start.cmp(&end) {
            Ordering::Equal => true,
            Ordering::Less => start <= now && now < end,
            Ordering::Greater => now >= start || now < end,
        }
    }

    /// How long until the window opens; `None` while it is open.
    pub fn until_open(&self, now: Time) -> Option<Duration> {
        if self.is_open(now) {
            return None;
        }
        let secs = seconds_of_day(self.start) - seconds_of_day(now);
        let secs = if secs > 0 { secs } else { secs + DAY };
        Some(Duration::from_secs(u64::try_from(secs).unwrap_or(0)))
    }
}

const DAY: i64 = 24 * 60 * 60;

fn seconds_of_day(t: Time) -> i64 {
    i64::from(t.hour()) * 3600 + i64::from(t.minute()) * 60 + i64::from(t.second())
}

/// `HH:MM`, 00:00 to 23:59.
pub fn parse_time(text: &str) -> Option<Time> {
    let (hour, minute) = text.split_once(':')?;
    let two_digits = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());
    if !two_digits(hour) || !two_digits(minute) {
        return None;
    }
    Time::new(hour.parse().ok()?, minute.parse().ok()?, 0, 0).ok()
}

#[cfg(test)]
mod tests {
    use jiff::civil::time;

    use super::*;

    fn window(start: &str, end: &str) -> Schedule {
        Schedule::new(parse_time(start).unwrap(), parse_time(end).unwrap())
    }

    #[test]
    fn a_window_inside_one_day_is_open_from_its_start_until_its_end() {
        let w = window("09:00", "17:30");
        assert!(!w.is_open(time(8, 59, 59, 0)));
        assert!(w.is_open(time(9, 0, 0, 0)));
        assert!(w.is_open(time(12, 0, 0, 0)));
        assert!(w.is_open(time(17, 29, 59, 0)));
        assert!(!w.is_open(time(17, 30, 0, 0)));
        assert!(!w.is_open(time(0, 0, 0, 0)));
        assert!(!w.is_open(time(23, 0, 0, 0)));
    }

    #[test]
    fn a_window_across_midnight_is_open_late_and_early() {
        let w = window("23:00", "07:00");
        assert!(w.is_open(time(23, 0, 0, 0)));
        assert!(w.is_open(time(23, 59, 59, 0)));
        assert!(w.is_open(time(0, 0, 0, 0)));
        assert!(w.is_open(time(6, 59, 0, 0)));
        assert!(!w.is_open(time(7, 0, 0, 0)));
        assert!(!w.is_open(time(12, 0, 0, 0)));
        assert!(!w.is_open(time(22, 59, 59, 0)));
    }

    #[test]
    fn a_window_starting_where_it_ends_is_open_all_day() {
        let w = window("08:00", "08:00");
        for hour in 0..24 {
            assert!(w.is_open(time(hour, 0, 0, 0)), "{hour}:00");
        }
        assert_eq!(w.until_open(time(8, 0, 0, 0)), None);
    }

    #[test]
    fn until_open_counts_to_the_next_start_wrapping_past_midnight() {
        let w = window("23:00", "07:00");
        assert_eq!(w.until_open(time(1, 0, 0, 0)), None);
        assert_eq!(
            w.until_open(time(22, 30, 0, 0)),
            Some(Duration::from_secs(30 * 60))
        );
        let day = window("09:00", "17:00");
        assert_eq!(
            day.until_open(time(18, 0, 0, 0)),
            Some(Duration::from_secs(15 * 3600))
        );
        assert_eq!(
            day.until_open(time(8, 59, 30, 0)),
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn times_are_hours_and_minutes() {
        assert_eq!(parse_time("07:05"), Some(time(7, 5, 0, 0)));
        assert_eq!(parse_time("23:59"), Some(time(23, 59, 0, 0)));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("7:5"), None);
        assert_eq!(parse_time("07:60"), None);
        assert_eq!(parse_time(""), None);
    }
}
