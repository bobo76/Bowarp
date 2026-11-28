//! The performance readout's numbers (frames per second, the slowest frame, this process's CPU
//! and memory) and its one line of text. The app measures frames and reads the platform's
//! process counters; everything computed from them is here.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How often the readout refreshes, and how far back its frame figures look.
pub const PERF_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// Shown for a figure not known yet, or not read on this platform.
const UNKNOWN: &str = "\u{2013}";
const BYTES_PER_MB: u64 = 1024 * 1024;
/// Windows `FILETIME` counts 100-nanosecond intervals.
const FILETIME_TICK_NANOS: u64 = 100;
/// Fields of `/proc/self/stat` after the command name: `utime` and `stime` are fields 14
/// and 15 of the whole line, the 12th and 13th after the closing parenthesis of field 2.
const STAT_UTIME_AFTER_NAME: usize = 11;

/// The frames drawn in the last [`PERF_SAMPLE_INTERVAL`], each with when it ended and how
/// long it took.
#[derive(Debug, Default)]
pub struct FrameWindow {
    frames: VecDeque<(Instant, Duration)>,
}

impl FrameWindow {
    pub fn record(&mut self, ended: Instant, took: Duration) {
        self.frames.push_back((ended, took));
    }

    /// The frames that ended in the interval before `now`; older ones are forgotten.
    pub fn summary(&mut self, now: Instant) -> FrameSummary {
        let expired = |&(ended, _): &(Instant, Duration)| {
            now.saturating_duration_since(ended) >= PERF_SAMPLE_INTERVAL
        };
        while self.frames.front().is_some_and(expired) {
            self.frames.pop_front();
        }
        FrameSummary {
            count: self.frames.len(),
            slowest: self.frames.iter().map(|&(_, took)| took).max(),
        }
    }
}

/// Frames in the last interval and the slowest of them (`None` without frames).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameSummary {
    pub count: usize,
    pub slowest: Option<Duration>,
}

/// What the platform reports about this process at one moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessCounters {
    /// User plus kernel CPU time since the process started.
    pub cpu_time: Duration,
    /// Resident memory (Windows working set, Linux RSS).
    pub resident_bytes: u64,
}

/// This process's CPU, as a percent of one core (may exceed 100), and its resident memory.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessUsage {
    pub cpu_percent: f64,
    pub resident_bytes: u64,
}

/// Turns successive [`ProcessCounters`] into [`ProcessUsage`]: CPU needs the previous read.
#[derive(Debug, Default)]
pub struct CpuMeter {
    last: Option<(Instant, Duration)>,
}

impl CpuMeter {
    /// The usage since the previous read; `None` for the first read or without counters.
    pub fn next(&mut self, at: Instant, counters: Option<ProcessCounters>) -> Option<ProcessUsage> {
        let counters = counters?;
        let previous = self.last.replace((at, counters.cpu_time));
        let (before, cpu_before) = previous?;
        Some(ProcessUsage {
            cpu_percent: cpu_percent(
                counters.cpu_time.saturating_sub(cpu_before),
                at.saturating_duration_since(before),
            ),
            resident_bytes: counters.resident_bytes,
        })
    }
}

/// CPU time used over a wall-clock span, as a percent of one core.
pub fn cpu_percent(cpu: Duration, wall: Duration) -> f64 {
    if wall.is_zero() {
        return 0.;
    }
    cpu.as_secs_f64() / wall.as_secs_f64() * 100.
}

/// The readout's line, as `0 fps · 0.0 ms max · 0.3% CPU · 182 MB`, each figure padded to a
/// fixed width so the line keeps its length. `frames` is `None` before the first refresh.
pub fn readout_text(frames: Option<FrameSummary>, usage: Option<ProcessUsage>) -> String {
    let fps = frames.map_or_else(|| UNKNOWN.to_owned(), |frames| frames.count.to_string());
    let slowest = frames
        .and_then(|frames| frames.slowest)
        .map_or_else(|| UNKNOWN.to_owned(), format_millis);
    let cpu = usage.map_or_else(
        || UNKNOWN.to_owned(),
        |usage| format!("{:.1}", usage.cpu_percent),
    );
    let megabytes = usage.map_or_else(
        || UNKNOWN.to_owned(),
        |usage| (usage.resident_bytes / BYTES_PER_MB).to_string(),
    );
    format!("{fps:>3} fps \u{b7} {slowest:>5} ms max \u{b7} {cpu:>5}% CPU \u{b7} {megabytes:>5} MB")
}

fn format_millis(took: Duration) -> String {
    format!("{:.1}", took.as_secs_f64() * 1000.)
}

/// A Windows `FILETIME` duration (100-nanosecond ticks, split in two halves).
pub fn filetime_duration(low: u32, high: u32) -> Duration {
    let ticks = (u64::from(high) << 32) | u64::from(low);
    Duration::from_nanos(ticks.saturating_mul(FILETIME_TICK_NANOS))
}

/// User plus system CPU time in clock ticks, from `/proc/self/stat`. The command name (field
/// 2) is in parentheses and may hold spaces or parentheses, so fields count from the last `)`.
pub fn proc_stat_cpu_ticks(stat: &str) -> Option<u64> {
    let (_, after_name) = stat.rsplit_once(')')?;
    let mut fields = after_name.split_whitespace().skip(STAT_UTIME_AFTER_NAME);
    let user: u64 = fields.next()?.parse().ok()?;
    let system: u64 = fields.next()?.parse().ok()?;
    Some(user.saturating_add(system))
}

/// Resident pages, the second field of `/proc/self/statm`.
pub fn proc_statm_resident_pages(statm: &str) -> Option<u64> {
    statm.split_whitespace().nth(1)?.parse().ok()
}

/// `ticks` of a clock running at `per_second`.
pub fn ticks_duration(ticks: u64, per_second: u64) -> Option<Duration> {
    if per_second == 0 {
        return None;
    }
    let whole = Duration::from_secs(ticks / per_second);
    Some(whole + Duration::from_secs(ticks % per_second) / u32::try_from(per_second).ok()?)
}

#[cfg(test)]
mod tests;
