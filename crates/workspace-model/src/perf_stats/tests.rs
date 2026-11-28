use super::*;

const MS: Duration = Duration::from_millis(1);

#[test]
fn frames_older_than_the_interval_are_forgotten() {
    let start = Instant::now();
    let mut window = FrameWindow::default();
    window.record(start, 9 * MS);
    window.record(start + 500 * MS, 4 * MS);
    window.record(start + 900 * MS, 2 * MS);
    let summary = window.summary(start + 999 * MS);
    assert_eq!(
        summary,
        FrameSummary {
            count: 3,
            slowest: Some(9 * MS)
        }
    );
    let summary = window.summary(start + 1000 * MS);
    assert_eq!(
        summary,
        FrameSummary {
            count: 2,
            slowest: Some(4 * MS)
        }
    );
    assert_eq!(window.summary(start + 3000 * MS), FrameSummary::default());
}

#[test]
fn cpu_needs_a_previous_read() {
    let start = Instant::now();
    let counters = |millis| {
        Some(ProcessCounters {
            cpu_time: millis * MS,
            resident_bytes: 5 * BYTES_PER_MB,
        })
    };
    let mut meter = CpuMeter::default();
    assert_eq!(meter.next(start, counters(100)), None);
    let usage = meter.next(start + 1000 * MS, counters(130)).unwrap();
    assert!(
        (usage.cpu_percent - 3.).abs() < 1e-9,
        "{}",
        usage.cpu_percent
    );
    assert_eq!(usage.resident_bytes, 5 * BYTES_PER_MB);
    // Without counters (no platform code) there is no usage, and the baseline stays.
    assert_eq!(meter.next(start + 2000 * MS, None), None);
}

#[test]
fn cpu_is_a_percent_of_one_core() {
    let second = Duration::from_secs(1);
    assert!((cpu_percent(250 * MS, second) - 25.).abs() < 1e-9);
    assert!((cpu_percent(2 * second, second) - 200.).abs() < 1e-9);
    assert!(cpu_percent(second, Duration::ZERO).abs() < f64::EPSILON);
}

#[test]
fn readout_text_keeps_its_width() {
    let idle = readout_text(Some(FrameSummary::default()), None);
    assert_eq!(
        idle,
        "  0 fps \u{b7}     \u{2013} ms max \u{b7}     \u{2013}% CPU \u{b7}     \u{2013} MB"
    );
    let busy = readout_text(
        Some(FrameSummary {
            count: 60,
            slowest: Some(Duration::from_micros(16_640)),
        }),
        Some(ProcessUsage {
            cpu_percent: 12.34,
            resident_bytes: 182 * BYTES_PER_MB + 7,
        }),
    );
    assert_eq!(
        busy,
        " 60 fps \u{b7}  16.6 ms max \u{b7}  12.3% CPU \u{b7}   182 MB"
    );
    assert_eq!(idle.chars().count(), busy.chars().count());
}

#[test]
fn readout_text_before_the_first_refresh_is_placeholders() {
    assert_eq!(
        readout_text(None, None),
        "  \u{2013} fps \u{b7}     \u{2013} ms max \u{b7}     \u{2013}% CPU \u{b7}     \u{2013} MB"
    );
}

#[test]
fn filetime_halves_make_100_nanosecond_ticks() {
    assert_eq!(filetime_duration(10_000_000, 0), Duration::from_secs(1));
    assert_eq!(
        filetime_duration(0, 1),
        Duration::from_nanos((1u64 << 32) * FILETIME_TICK_NANOS)
    );
}

#[test]
fn proc_stat_counts_fields_after_the_command_name() {
    let stat = "1234 (my (odd) app) S 1 1234 1234 0 -1 4194560 100 0 0 0 250 75 0 0 20 0 9 0 \
                5000 100000 300 18446744073709551615";
    assert_eq!(proc_stat_cpu_ticks(stat), Some(325));
    assert_eq!(proc_stat_cpu_ticks("1234 (app) S 1 2"), None);
    assert_eq!(proc_stat_cpu_ticks("no name"), None);
}

#[test]
fn proc_statm_reads_resident_pages() {
    assert_eq!(
        proc_statm_resident_pages("12000 4500 800 10 0 3000 0\n"),
        Some(4500)
    );
    assert_eq!(proc_statm_resident_pages("12000"), None);
    assert_eq!(proc_statm_resident_pages("12000 x"), None);
}

#[test]
fn ticks_turn_into_durations() {
    assert_eq!(ticks_duration(250, 100), Some(Duration::from_millis(2500)));
    assert_eq!(ticks_duration(1, 3), Some(Duration::from_secs(1) / 3));
    assert_eq!(ticks_duration(5, 0), None);
}
