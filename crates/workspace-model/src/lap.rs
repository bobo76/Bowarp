//! Lap time: how long a session has been in its current status, as the hover card shows it
//! ("Working · 2m 14s").

use std::time::{Duration, Instant};

use crate::time::{SECONDS_PER_HOUR, SECONDS_PER_MINUTE};

/// A status and when it started. Setting the same status again keeps the lap running; a
/// different one starts a new lap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lap<S> {
    status: S,
    since: Instant,
}

impl<S: Copy + PartialEq> Lap<S> {
    pub fn new(status: S, now: Instant) -> Self {
        Self { status, since: now }
    }

    pub fn status(&self) -> S {
        self.status
    }

    /// Record the status as of `now`. Returns whether it changed (and a new lap started).
    pub fn set(&mut self, status: S, now: Instant) -> bool {
        if status == self.status {
            return false;
        }
        *self = Self::new(status, now);
        true
    }

    /// How long the status has lasted at `now`.
    pub fn elapsed(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.since)
    }
}

/// How long until `elapsed` reaches its next whole second, when a ticking display changes.
pub fn until_next_second(elapsed: Duration) -> Duration {
    Duration::from_secs(1) - Duration::from_nanos(u64::from(elapsed.subsec_nanos()))
}

/// A lap time in whole seconds: `45s`, `2m 14s`, `1h 05m`. Seconds are dropped from an hour on,
/// where they no longer matter.
pub fn format_lap(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    let (hours, minutes) = (
        seconds / SECONDS_PER_HOUR,
        seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE,
    );
    match (hours, minutes) {
        (0, 0) => format!("{seconds}s"),
        (0, _) => format!("{minutes}m {:02}s", seconds % SECONDS_PER_MINUTE),
        _ => format!("{hours}h {minutes:02}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Status {
        Idle,
        Working,
    }

    #[test]
    fn same_status_keeps_the_lap_running() {
        let start = Instant::now();
        let mut lap = Lap::new(Status::Working, start);
        assert!(!lap.set(Status::Working, start + Duration::from_secs(5)));
        assert_eq!(
            lap.elapsed(start + Duration::from_secs(8)),
            Duration::from_secs(8)
        );
    }

    #[test]
    fn a_new_status_starts_a_new_lap() {
        let start = Instant::now();
        let mut lap = Lap::new(Status::Idle, start);
        assert!(lap.set(Status::Working, start + Duration::from_secs(3)));
        assert_eq!(lap.status(), Status::Working);
        assert_eq!(
            lap.elapsed(start + Duration::from_secs(10)),
            Duration::from_secs(7)
        );
    }

    #[test]
    fn elapsed_before_the_start_is_zero() {
        let start = Instant::now() + Duration::from_secs(1);
        assert_eq!(
            Lap::new(Status::Idle, start).elapsed(Instant::now()),
            Duration::ZERO
        );
    }

    #[test]
    fn ticks_on_whole_seconds() {
        assert_eq!(until_next_second(Duration::ZERO), Duration::from_secs(1));
        assert_eq!(
            until_next_second(Duration::from_millis(2_300)),
            Duration::from_millis(700)
        );
    }

    #[test]
    fn formats_seconds_minutes_and_hours() {
        let format = |seconds| format_lap(Duration::from_secs(seconds));
        assert_eq!(format(0), "0s");
        assert_eq!(format(45), "45s");
        assert_eq!(format(60), "1m 00s");
        assert_eq!(format(134), "2m 14s");
        assert_eq!(format(3599), "59m 59s");
        assert_eq!(format(3600), "1h 00m");
        assert_eq!(format(3600 * 26 + 5 * 60 + 59), "26h 05m");
        assert_eq!(format_lap(Duration::from_millis(59_999)), "59s");
    }
}
