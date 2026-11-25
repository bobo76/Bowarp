//! The title bar's performance readout: this window's frames per second and slowest frame,
//! and this process's CPU and memory, refreshed once a second while the "Performance readout"
//! switch is on. Off, nothing of it exists: no entity, no timer.
//!
//! Each window has a [`PerfReadout`] fed by its workspace's frames ([`FrameMeter`]); one
//! sampler per process reads the CPU and memory counters off the UI thread and publishes them
//! once a second, which is when every readout refreshes.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Instant;

use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    AnyElement, App, AppContext, AsyncApp, Context, Entity, Global, IntoElement, ParentElement,
    Render, StyleRefinement, Styled, Subscription, Task, Window, canvas, div, rems,
};
use workspace_model::{
    CpuMeter, FrameSummary, FrameWindow, PERF_SAMPLE_INTERVAL, ProcessCounters, ProcessUsage,
    readout_text,
};

/// The readout's width, for its longest line (46 characters of a monospace font at
/// `text_xs`, about 0.6em each) with some room: a cached view needs a definite size.
const READOUT_WIDTH_REMS: f32 = 22.;
const READOUT_MARGIN_REMS: f32 = 0.5;

/// What a workspace keeps while its readout is on: dropping it drops the readout and, with
/// the last one in the process, the sampler's timer.
pub struct PerfReadoutHandle {
    readout: Entity<PerfReadout>,
    frames: Rc<FrameMeter>,
}

impl PerfReadoutHandle {
    pub fn new(cx: &mut App) -> Self {
        let frames = Rc::new(FrameMeter::default());
        let shared = frames.clone();
        let readout = cx.new(|cx| PerfReadout::new(shared, cx));
        Self { readout, frames }
    }

    /// The readout for the title bar, as a cached view: the window's other frames reuse its
    /// last drawing. Its own refresh still re-renders the views above it (GPUI marks a
    /// notified view's ancestors dirty), which is why that frame isn't counted.
    pub fn view(&self) -> AnyElement {
        let style = StyleRefinement::default()
            .w(rems(READOUT_WIDTH_REMS))
            .h_full()
            .flex_shrink_0()
            .mr(rems(READOUT_MARGIN_REMS));
        self.readout.clone().cached(style).into_any_element()
    }

    /// Frames counted in the last second.
    #[cfg(test)]
    pub fn frames_counted(&self) -> usize {
        self.frames.summary().count
    }

    /// An invisible element that records the frame begun at `started` once it is painted.
    /// Placed last in the workspace, so the frame's own render, layout and paint come first.
    pub fn frame_probe(&self, started: Instant) -> AnyElement {
        let frames = self.frames.clone();
        canvas(|_, _, _| {}, move |_, (), _, _| frames.frame_drawn(started))
            .absolute()
            .into_any_element()
    }
}

/// One window's frames over the last second, shared by its workspace (which records them)
/// and its readout (which shows them).
#[derive(Default)]
pub struct FrameMeter {
    frames: RefCell<FrameWindow>,
    /// The readout asked for a frame to show new text: that frame is not counted, so an idle
    /// window reads 0 fps.
    own_refresh: Cell<bool>,
}

impl FrameMeter {
    fn frame_drawn(&self, started: Instant) {
        if self.own_refresh.replace(false) {
            return;
        }
        let now = Instant::now();
        self.frames
            .borrow_mut()
            .record(now, now.saturating_duration_since(started));
    }

    fn summary(&self) -> FrameSummary {
        self.frames.borrow_mut().summary(Instant::now())
    }
}

pub struct PerfReadout {
    frames: Rc<FrameMeter>,
    /// The line drawn; placeholders until the first sample.
    shown: String,
    /// The sampler's `published` count last shown.
    seen: u64,
    _sampler: Rc<SamplerTask>,
    _samples: Subscription,
}

impl PerfReadout {
    fn new(frames: Rc<FrameMeter>, cx: &mut Context<Self>) -> Self {
        let sampler = ProcessSampler::acquire(cx);
        Self {
            frames,
            shown: readout_text(None, None),
            seen: ProcessSampler::published(cx),
            _sampler: sampler,
            _samples: cx.observe_global::<ProcessSampler>(Self::refresh),
        }
    }

    /// A new sample: show it with this window's frames, redrawing only if the line changed.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let sampler = cx.global::<ProcessSampler>();
        if sampler.published == self.seen {
            return;
        }
        self.seen = sampler.published;
        let text = readout_text(Some(self.frames.summary()), sampler.latest);
        if text == self.shown {
            return;
        }
        self.shown = text;
        self.frames.own_refresh.set(true);
        cx.notify();
    }
}

impl Render for PerfReadout {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_end()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_xs()
            .font_family(cx.theme().mono_font_family.clone())
            .text_color(cx.theme().muted_foreground)
            .child(self.shown.clone())
    }
}

/// The process's one sampler: the latest CPU and memory usage, published once a second while
/// any readout holds the task.
#[derive(Default)]
struct ProcessSampler {
    task: Weak<SamplerTask>,
    latest: Option<ProcessUsage>,
    /// How many samples were published, so readouts refresh once per sample.
    published: u64,
}

impl Global for ProcessSampler {}

/// The sampling loop; dropped (cancelled) with the last readout holding it.
struct SamplerTask {
    _task: Task<()>,
}

impl ProcessSampler {
    /// The running sampler, started if no readout holds one.
    fn acquire(cx: &mut App) -> Rc<SamplerTask> {
        let running = cx
            .try_global::<Self>()
            .and_then(|sampler| sampler.task.upgrade());
        if let Some(task) = running {
            return task;
        }
        let task = Rc::new(SamplerTask {
            _task: cx.spawn(async move |cx| sample(cx).await),
        });
        let published = Self::published(cx);
        cx.set_global(Self {
            task: Rc::downgrade(&task),
            latest: None,
            published,
        });
        task
    }

    fn published(cx: &App) -> u64 {
        cx.try_global::<Self>()
            .map_or(0, |sampler| sampler.published)
    }
}

/// Reads the counters off the UI thread once a second; the first read is only the baseline
/// CPU usage is measured from.
async fn sample(cx: &mut AsyncApp) {
    let mut meter = CpuMeter::default();
    let (at, counters) = read_counters_off_thread(cx).await;
    meter.next(at, counters);
    loop {
        cx.background_executor().timer(PERF_SAMPLE_INTERVAL).await;
        let (at, counters) = read_counters_off_thread(cx).await;
        let usage = meter.next(at, counters);
        cx.update(|cx| {
            let sampler = cx.global_mut::<ProcessSampler>();
            sampler.latest = usage;
            sampler.published += 1;
        });
    }
}

async fn read_counters_off_thread(cx: &AsyncApp) -> (Instant, Option<ProcessCounters>) {
    cx.background_spawn(async { (Instant::now(), read_counters()) })
        .await
}

/// This process's CPU time (`GetProcessTimes`) and working set (`GetProcessMemoryInfo`).
#[cfg(windows)]
fn read_counters() -> Option<ProcessCounters> {
    Some(ProcessCounters {
        cpu_time: windows::cpu_time()?,
        resident_bytes: windows::working_set()?,
    })
}

/// This process's CPU time and resident memory, from `/proc/self/stat` and `statm`.
#[cfg(target_os = "linux")]
fn read_counters() -> Option<ProcessCounters> {
    use workspace_model::{proc_stat_cpu_ticks, proc_statm_resident_pages, ticks_duration};

    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    // SAFETY: `sysconf` has no memory-safety preconditions; failure is -1, rejected below.
    let ticks_per_second = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    // SAFETY: as above.
    let page_size = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
    Some(ProcessCounters {
        cpu_time: ticks_duration(proc_stat_cpu_ticks(&stat)?, ticks_per_second)?,
        resident_bytes: proc_statm_resident_pages(&statm)?.saturating_mul(page_size),
    })
}

/// No counters on other platforms (macOS): the readout shows placeholders for them.
#[cfg(not(any(windows, target_os = "linux")))]
fn read_counters() -> Option<ProcessCounters> {
    None
}

#[cfg(windows)]
mod windows {
    use std::time::Duration;

    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    use workspace_model::filetime_duration;

    pub(super) fn cpu_time() -> Option<Duration> {
        let mut unused = (FILETIME::default(), FILETIME::default());
        let (mut kernel, mut user) = (FILETIME::default(), FILETIME::default());
        // SAFETY: `GetCurrentProcess` returns a pseudo handle that needs no closing; the four
        // pointers are to locals that outlive the call.
        let read = unsafe {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut unused.0,
                &mut unused.1,
                &mut kernel,
                &mut user,
            )
        };
        (read != 0).then(|| duration(kernel) + duration(user))
    }

    fn duration(time: FILETIME) -> Duration {
        filetime_duration(time.dwLowDateTime, time.dwHighDateTime)
    }

    pub(super) fn working_set() -> Option<u64> {
        let mut counters = PROCESS_MEMORY_COUNTERS::default();
        let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
        // SAFETY: as above for the handle; `counters` is a live local of `size` bytes.
        let read = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
        (read != 0).then_some(counters.WorkingSetSize as u64)
    }
}

#[cfg(test)]
mod tests;
