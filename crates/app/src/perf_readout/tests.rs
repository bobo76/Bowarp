use super::*;

use std::time::Duration;

use gpui_kit::TestAppContext;

fn sampler_running(cx: &App) -> bool {
    cx.try_global::<ProcessSampler>()
        .is_some_and(|sampler| sampler.task.upgrade().is_some())
}

#[test]
fn the_readouts_own_refresh_frame_is_not_counted() {
    let meter = FrameMeter::default();
    meter.frame_drawn(Instant::now());
    meter.own_refresh.set(true);
    meter.frame_drawn(Instant::now());
    assert_eq!(meter.summary().count, 1);
    meter.frame_drawn(Instant::now());
    assert_eq!(meter.summary().count, 2);
    assert!(meter.summary().slowest.is_some());
}

#[gpui::test]
fn placeholders_show_until_the_first_sample_a_second_later(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let handle = cx.update(PerfReadoutHandle::new);
    let shown = |cx: &mut TestAppContext| {
        handle
            .readout
            .read_with(cx, |readout, _| readout.shown.clone())
    };
    let placeholders = readout_text(None, None);
    cx.run_until_parked();
    assert_eq!(shown(cx), placeholders);
    cx.executor()
        .advance_clock(PERF_SAMPLE_INTERVAL - Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(shown(cx), placeholders);
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    let first = shown(cx);
    assert!(first.starts_with("  0 fps"), "{first}");
    assert!(
        handle.frames.own_refresh.get(),
        "the redraw it asked for is not counted"
    );
    if cfg!(any(windows, target_os = "linux")) {
        assert!(
            first.ends_with(" MB") && !first.ends_with("\u{2013} MB"),
            "{first}"
        );
    }
}

#[gpui::test]
fn a_sample_that_leaves_the_line_unchanged_asks_for_no_redraw(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let handle = cx.update(PerfReadoutHandle::new);
    let publish_unread_sample = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let sampler = cx.global_mut::<ProcessSampler>();
            sampler.latest = None;
            sampler.published += 1;
        });
    };
    publish_unread_sample(cx);
    assert!(
        handle.frames.own_refresh.replace(false),
        "the first line replaces the placeholders"
    );
    publish_unread_sample(cx);
    assert!(!handle.frames.own_refresh.get(), "same line: no redraw");
    assert_eq!(
        handle
            .readout
            .read_with(cx, |readout, _| readout.shown.clone()),
        readout_text(Some(FrameSummary::default()), None)
    );
}

#[gpui::test]
fn one_sampler_runs_while_any_readout_holds_it(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let first = cx.update(PerfReadoutHandle::new);
    let second = cx.update(PerfReadoutHandle::new);
    cx.update(|cx| {
        assert!(Rc::ptr_eq(
            &first.readout.read(cx)._sampler,
            &second.readout.read(cx)._sampler
        ));
    });
    drop(first);
    // Dropped entities are released when effects are next flushed.
    cx.update(|_| {});
    assert!(cx.update(|cx| sampler_running(cx)));
    drop(second);
    cx.update(|_| {});
    assert!(!cx.update(|cx| sampler_running(cx)));
    // Turned on again: a new sampler, which still publishes after a second.
    let again = cx.update(PerfReadoutHandle::new);
    let published = cx.update(|cx| ProcessSampler::published(cx));
    cx.executor().advance_clock(PERF_SAMPLE_INTERVAL);
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| ProcessSampler::published(cx)), published + 1);
    drop(again);
}
