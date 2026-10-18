use std::borrow::Cow;
use std::io;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_status::{AgentState, AgentStatus, HookEvent};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui_kit::{
    App, AppContext, Bounds, ClipboardItem, Context, CursorStyle, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyBinding,
    KeyDownEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    NoAction, ParentElement, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent, Styled, Task,
    UTF16Selection, Window, actions, div, size,
};
use term_core::alacritty_terminal::grid::Scroll;
use term_core::alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use term_core::alacritty_terminal::selection::SelectionType;
use term_core::alacritty_terminal::term::TermMode;
use term_core::{
    EventCallback, LaunchSpec, Palette, Session, SessionEvent, SessionKind, TermDimensions,
};
use workspace_model::{Lap, Location};

use crate::keys;
use crate::launcher;
use crate::mouse::{self, Cell, MouseAction, ReportButton, WheelAction};
use crate::scrollbar::{self, ScrollbarLayout, ScrollbarState};
use crate::sidebar::TabStatus;
use crate::terminal_element::{TerminalElement, to_hsla};

const KEY_CONTEXT: &str = "Terminal";

/// How long after its last output a session without status hooks still counts as active.
const ACTIVITY_WINDOW: Duration = Duration::from_secs(2);

/// Keys bound by the window root (focus traversal, copy) that terminal programs need: Tab for
/// completion, Shift+Tab for Claude Code's mode cycling, Ctrl+C for interrupt.
const PASSTHROUGH_KEYS: &[&str] = &["tab", "shift-tab", "ctrl-c"];

actions!(terminal, [Copy, Paste]);

pub fn init(cx: &mut App) {
    cx.bind_keys(
        PASSTHROUGH_KEYS
            .iter()
            .map(|key| KeyBinding::new(key, NoAction, Some(KEY_CONTEXT))),
    );
    cx.bind_keys(default_bindings());
}

/// Terminal shortcuts. Bindings match before `on_key_down`, so these keys never reach the
/// program. Kept as data so a user keymap can replace them later.
fn default_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("ctrl-shift-c", Copy, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-insert", Copy, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-v", Paste, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-insert", Paste, Some(KEY_CONTEXT)),
    ]
}

/// Cell geometry from the last paint, used to map pointer positions to grid cells.
#[derive(Clone, Copy, Debug)]
pub struct GridGeometry {
    pub origin: Point<Pixels>,
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub columns: usize,
    pub lines: usize,
    /// Top-left of the cursor cell, for placing IME candidate windows.
    pub cursor_origin: Point<Pixels>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessState {
    Running,
    /// The child exited, with its exit code when one is available.
    Exited(Option<i32>),
}

/// Emitted when what the sidebar and title bar show changes: title, status, activity, folder
/// or branch.
pub struct TabChanged;

/// One terminal session and its interaction state.
pub struct TerminalView {
    pub(crate) session: Session,
    /// Shown until the program sets a title.
    name: String,
    /// Folder the session was started in, used while the child's own cannot be read.
    launch_cwd: Option<PathBuf>,
    location: Option<Location>,
    /// Status from Claude Code hooks; `None` for sessions without hooks.
    agent: Option<AgentState>,
    /// When the hook status last changed, for the hover card's lap time.
    agent_lap: Lap<AgentStatus>,
    /// Claude Code finished a turn while the tab was hidden; cleared when it is shown.
    finished_unseen: bool,
    /// Hidden views do not paint, so they acknowledge wakeups themselves.
    visible: bool,
    /// Runs while the session counts as active, to report when it goes quiet.
    activity_task: Option<Task<()>>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) geometry: Option<GridGeometry>,
    /// IME composition text not yet committed.
    pub(crate) marked_text: Option<String>,
    title: Option<String>,
    process: ProcessState,
    scroll_remainder: f32,
    selecting: bool,
    /// Button whose press was reported to the program; its release and drags are reported too.
    reported_button: Option<ReportButton>,
    /// Last cell a motion report was sent for, so motion is reported once per cell.
    last_motion_cell: Option<Cell>,
    pub(crate) scrollbar: ScrollbarState,
    _session_events: Task<()>,
}

impl TerminalView {
    /// Start `spec` as session `id`. `reports_status` says whether the session was set up
    /// with Claude Code status hooks.
    pub fn spawn(
        id: u64,
        spec: &LaunchSpec,
        reports_status: bool,
        cx: &mut App,
    ) -> io::Result<Entity<Self>> {
        let (sender, mut receiver) = mpsc::unbounded();
        let callback: EventCallback = Arc::new(move |event| {
            let _ = sender.unbounded_send(event);
        });
        let session = Session::spawn(
            id,
            spec,
            TermDimensions::new(80, 24),
            Palette::default(),
            callback,
        )?;

        Ok(cx.new(|cx| {
            let session_events = cx.spawn(async move |this, cx| {
                while let Some(event) = receiver.next().await {
                    if this
                        .update(cx, |view: &mut Self, cx| view.on_session_event(event, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            Self::new(session, spec, reports_status, session_events, cx)
        }))
    }

    fn new(
        session: Session,
        spec: &LaunchSpec,
        reports_status: bool,
        session_events: Task<()>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            session,
            name: launcher::default_name(spec),
            launch_cwd: spec.cwd.clone(),
            location: spec.cwd.clone().map(Location::read),
            agent: reports_status.then(AgentState::default),
            agent_lap: Lap::new(AgentState::default().status(), Instant::now()),
            finished_unseen: false,
            visible: true,
            activity_task: None,
            focus_handle: cx.focus_handle(),
            geometry: None,
            marked_text: None,
            title: None,
            process: ProcessState::Running,
            scroll_remainder: 0.0,
            selecting: false,
            reported_button: None,
            last_motion_cell: None,
            scrollbar: ScrollbarState::default(),
            _session_events: session_events,
        }
    }

    pub fn id(&self) -> u64 {
        self.session.id()
    }

    pub fn kind(&self) -> SessionKind {
        self.session.kind()
    }

    /// Where the session works now, as of the last tab change.
    pub fn location(&self) -> Option<&Location> {
        self.location.as_ref()
    }

    /// The child's folder now, else the last one known, else the one it started in.
    pub fn current_folder(&self) -> Option<PathBuf> {
        self.session
            .current_dir()
            .or_else(|| self.location.as_ref().map(|location| location.dir.clone()))
            .or_else(|| self.launch_cwd.clone())
    }

    /// The program's title, or the session's default name.
    pub fn name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }

    pub fn display_title(&self) -> String {
        let title = self.name();
        match self.process {
            ProcessState::Running => title.to_string(),
            ProcessState::Exited(Some(code)) => format!("{title} [exited {code}]"),
            ProcessState::Exited(None) => format!("{title} [exited]"),
        }
    }

    pub fn tab_status(&self) -> TabStatus {
        if let Some(agent) = self.agent {
            return match agent.status() {
                AgentStatus::Idle if self.finished_unseen => TabStatus::Done,
                AgentStatus::Idle => TabStatus::Idle,
                AgentStatus::Working => TabStatus::Working,
                AgentStatus::NeedsAttention => TabStatus::NeedsAttention,
                AgentStatus::Exited => TabStatus::Exited,
            };
        }
        match self.process {
            ProcessState::Exited(_) => TabStatus::Exited,
            ProcessState::Running if self.activity_task.is_some() => TabStatus::Active,
            ProcessState::Running => TabStatus::Quiet,
        }
    }

    /// How long Claude Code has been working on the current turn, or `None` when it is not
    /// working. Tool calls keep the lap running; waiting for a permission decision ends it.
    pub fn working_for(&self, now: Instant) -> Option<Duration> {
        let working = self.agent?.status() == AgentStatus::Working;
        working.then(|| self.agent_lap.elapsed(now))
    }

    /// Stop painting the view; only the selected tab is painted.
    pub fn hide(&mut self) {
        self.visible = false;
        // A wakeup sent before the next paint would never be acknowledged by `prepaint`.
        self.session.ack_wakeup();
    }

    /// Paint the view again, clearing a finished turn it showed while hidden.
    pub fn show(&mut self, cx: &mut Context<Self>) {
        self.visible = true;
        let was_done = std::mem::take(&mut self.finished_unseen);
        if self.refresh_location() || was_done {
            cx.emit(TabChanged);
        }
        cx.notify();
    }

    /// Re-read the child's folder and its git branch. Returns whether either changed. When the
    /// folder cannot be read (exited child, unsupported platform) the last known one is kept.
    fn refresh_location(&mut self) -> bool {
        let Some(dir) = self.current_folder() else {
            return false;
        };
        let location = Some(Location::read(dir));
        if location == self.location {
            return false;
        }
        self.location = location;
        true
    }

    /// Tell the sidebar something changed, refreshing the folder and branch first: a shell's
    /// title changes and output bursts end around `cd` and `git checkout`, and hook events
    /// follow Claude Code's commands, so the location is read then instead of polled.
    fn emit_tab_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh_location();
        cx.emit(TabChanged);
    }

    /// Apply a status event from this session's Claude Code hooks.
    pub fn apply_hook_event(&mut self, event: HookEvent, at: u64, cx: &mut Context<Self>) {
        let Some(agent) = self.agent.as_mut() else {
            return;
        };
        let before = agent.status();
        agent.apply(event, at);
        let after = agent.status();
        if !self.agent_lap.set(after, Instant::now()) {
            return;
        }
        self.finished_unseen = finished_unseen(before, after, self.visible);
        self.emit_tab_changed(cx);
    }

    fn on_session_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        match event {
            SessionEvent::Wakeup => {
                self.track_activity(cx);
                if !self.visible {
                    // Nothing paints, so re-arm here to keep hearing about output.
                    self.session.ack_wakeup();
                    return;
                }
            }
            SessionEvent::TitleChanged(title) => {
                self.title = title;
                self.emit_tab_changed(cx);
            }
            SessionEvent::Bell => return,
            SessionEvent::Exited(code) => {
                self.process = ProcessState::Exited(code);
                if let Some(agent) = self.agent.as_mut() {
                    agent.exit();
                    self.agent_lap.set(agent.status(), Instant::now());
                }
                cx.emit(TabChanged);
            }
        }
        cx.notify();
    }

    /// Mark a session without status hooks active on output, and quiet again once no output
    /// arrived for [`ACTIVITY_WINDOW`].
    fn track_activity(&mut self, cx: &mut Context<Self>) {
        if self.agent.is_some() || self.activity_task.is_some() {
            return;
        }
        self.activity_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some(last_output)) =
                    this.read_with(cx, |view, _| view.session.last_output())
                else {
                    return;
                };
                let quiet_at = last_output + ACTIVITY_WINDOW;
                let now = Instant::now();
                if now >= quiet_at {
                    break;
                }
                cx.background_executor().timer(quiet_at - now).await;
            }
            let _ = this.update(cx, |view, cx| {
                view.activity_task = None;
                view.emit_tab_changed(cx);
            });
        }));
        cx.emit(TabChanged);
    }

    /// Send user input to the program, snapping the view back to the live screen.
    fn send_input(&mut self, bytes: Cow<'static, [u8]>, cx: &mut Context<Self>) {
        self.session.scroll_display(Scroll::Bottom);
        self.session.clear_selection();
        self.session.write(bytes);
        cx.notify();
    }

    fn mode(&self) -> TermMode {
        self.session.mode()
    }

    fn on_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.session.selection_text();
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return;
        };
        let bytes = keys::paste_bytes(&text, self.mode());
        self.send_input(bytes.into(), cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let consumed =
            self.handle_scrollback_shortcut(&event.keystroke, cx) || self.forward_key(event, cx);
        if consumed {
            cx.stop_propagation();
        }
    }

    /// Shift+PageUp and Shift+PageDown move through the scrollback history.
    fn handle_scrollback_shortcut(
        &mut self,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if !is_shift_only(&keystroke.modifiers) {
            return false;
        }
        let scroll = match keystroke.key.as_str() {
            "pageup" => Scroll::PageUp,
            "pagedown" => Scroll::PageDown,
            _ => return false,
        };
        self.scroll_scrollback(scroll, cx);
        true
    }

    /// Move through the scrollback on the user's request, showing the scroll bar.
    fn scroll_scrollback(&mut self, scroll: Scroll, cx: &mut Context<Self>) {
        self.session.scroll_display(scroll);
        self.reveal_scrollbar(cx);
        cx.notify();
    }

    /// Show the scroll bar until it has been left alone for [`scrollbar::LINGER`].
    fn reveal_scrollbar(&mut self, cx: &mut Context<Self>) {
        self.scrollbar.revealed = true;
        self.scrollbar.hide_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(scrollbar::LINGER).await;
            let _ = this.update(cx, |view, cx| {
                view.scrollbar.revealed = false;
                cx.notify();
            });
        }));
    }

    /// Drop hover and drag state once there is no history to scroll (e.g. on the alternate
    /// screen), since the bar and its listeners are gone.
    pub(crate) fn set_scrollable(&mut self, scrollable: bool, cx: &mut Context<Self>) {
        if !scrollable && self.scrollbar.is_active() {
            self.scrollbar.hovered = false;
            self.scrollbar.drag_grab = None;
            cx.notify();
        }
    }

    /// A left press on the track starts a thumb drag; off the thumb it first centers the thumb
    /// on the pointer. Returns whether the press was consumed.
    pub(crate) fn scrollbar_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        layout: ScrollbarLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.button != MouseButton::Left || !layout.track.contains(&event.position) {
            return false;
        }
        let Some((top, height)) = layout.thumb() else {
            return false;
        };
        window.focus(&self.focus_handle, cx);
        let y = f32::from(event.position.y - layout.track.origin.y);
        let grab = if (top..top + height).contains(&y) {
            y - top
        } else {
            height / 2.0
        };
        self.scrollbar.drag_grab = Some(grab);
        self.session
            .scroll_to(layout.offset_for_pointer(event.position.y, grab));
        cx.notify();
        true
    }

    /// Drags the thumb, or tracks hover over the track. Returns whether the move was consumed.
    pub(crate) fn scrollbar_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        layout: ScrollbarLayout,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(grab) = self.scrollbar.drag_grab {
            self.session
                .scroll_to(layout.offset_for_pointer(event.position.y, grab));
            cx.notify();
            return true;
        }
        let hovered = layout.track.contains(&event.position);
        if hovered != self.scrollbar.hovered {
            self.scrollbar.hovered = hovered;
            if !hovered {
                self.reveal_scrollbar(cx);
            }
            cx.notify();
        }
        false
    }

    /// Ends a thumb drag. Returns whether the release was consumed.
    pub(crate) fn scrollbar_mouse_up(&mut self, cx: &mut Context<Self>) -> bool {
        if self.scrollbar.drag_grab.take().is_none() {
            return false;
        }
        self.reveal_scrollbar(cx);
        cx.notify();
        true
    }

    fn forward_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let keystroke = &event.keystroke;
        let Some(bytes) = keys::key_to_bytes(keystroke, event.prefer_character_input, self.mode())
        else {
            return false;
        };
        self.send_input(bytes, cx);
        true
    }

    fn on_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        self.paste(cx.read_from_clipboard().and_then(|item| item.text()), cx);
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(geometry) = self.geometry else {
            return;
        };
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / f32::from(geometry.line_height),
        };
        self.scroll_remainder += lines;
        let whole = self.scroll_remainder.trunc() as i32;
        if whole == 0 {
            return;
        }
        self.scroll_remainder -= whole as f32;

        let Some((cell, _)) = self.viewport_cell(event.position) else {
            return;
        };
        match mouse::wheel_action(whole, cell, &event.modifiers, self.mode()) {
            WheelAction::Program(bytes) => self.session.write(bytes),
            WheelAction::Scrollback(lines) => self.scroll_scrollback(Scroll::Delta(lines), cx),
        }
        cx.notify();
        cx.stop_propagation();
    }

    /// The visible cell under `position`, clamped to the grid, and which half of it was hit.
    fn viewport_cell(&self, position: Point<Pixels>) -> Option<(Cell, Side)> {
        let geometry = self.geometry?;
        let cell_width = f32::from(geometry.cell_width);
        let line_height = f32::from(geometry.line_height);
        let x = f32::from(position.x - geometry.origin.x).max(0.0);
        let y = f32::from(position.y - geometry.origin.y).max(0.0);
        let column = ((x / cell_width) as usize).min(geometry.columns.saturating_sub(1));
        let line = ((y / line_height) as usize).min(geometry.lines.saturating_sub(1));
        let side = if x - column as f32 * cell_width < cell_width / 2.0 {
            Side::Left
        } else {
            Side::Right
        };
        Some((Cell { column, line }, side))
    }

    fn grid_point(&self, position: Point<Pixels>) -> Option<(GridPoint, Side)> {
        let (cell, side) = self.viewport_cell(position)?;
        let display_offset = self.session.display_offset();
        Some((
            GridPoint::new(
                Line(cell.line as i32 - display_offset as i32),
                Column(cell.column),
            ),
            side,
        ))
    }

    fn send_mouse_report(
        &mut self,
        button: ReportButton,
        action: MouseAction,
        position: Point<Pixels>,
        modifiers: &Modifiers,
    ) {
        let Some((cell, _)) = self.viewport_cell(position) else {
            return;
        };
        if action == MouseAction::Motion {
            if self.last_motion_cell == Some(cell) {
                return;
            }
            self.last_motion_cell = Some(cell);
        }
        if let Some(bytes) = mouse::report(button, action, cell, modifiers, self.mode()) {
            self.session.write(bytes);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let Some(button) = ReportButton::from_gpui(event.button) else {
            return;
        };
        if mouse::is_reporting_to_program(self.mode(), &event.modifiers) {
            self.send_mouse_report(button, MouseAction::Press, event.position, &event.modifiers);
            self.reported_button = Some(button);
            return;
        }
        match event.button {
            MouseButton::Left => self.start_selection(event, cx),
            #[cfg(target_os = "linux")]
            MouseButton::Middle => {
                self.paste(cx.read_from_primary().and_then(|item| item.text()), cx);
            }
            _ => {}
        }
    }

    fn start_selection(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some((point, side)) = self.grid_point(event.position) else {
            return;
        };
        let selection_type = match event.click_count {
            2 => SelectionType::Semantic,
            3.. => SelectionType::Lines,
            _ => SelectionType::Simple,
        };
        self.session.start_selection(selection_type, point, side);
        self.selecting = true;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.reported_button.is_some()
            || mouse::is_reporting_to_program(self.mode(), &event.modifiers)
        {
            let button = self.reported_button.unwrap_or(ReportButton::None);
            self.send_mouse_report(
                button,
                MouseAction::Motion,
                event.position,
                &event.modifiers,
            );
            return;
        }
        if !self.selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some((point, side)) = self.grid_point(event.position) else {
            return;
        };
        self.session.update_selection(point, side);
        cx.notify();
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let button = ReportButton::from_gpui(event.button);
        if button.is_some() && button == self.reported_button {
            self.reported_button = None;
            self.send_mouse_report(
                button.unwrap_or(ReportButton::None),
                MouseAction::Release,
                event.position,
                &event.modifiers,
            );
            return;
        }
        if !self.selecting || event.button != MouseButton::Left {
            return;
        }
        self.selecting = false;
        let text = self.session.finish_selection();
        #[cfg(target_os = "linux")]
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = text;
        cx.notify();
    }
}

fn is_shift_only(modifiers: &Modifiers) -> bool {
    modifiers.shift && !modifiers.control && !modifiers.alt
}

impl EventEmitter<TabChanged> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = self.session.palette().background;
        let view = div()
            .size_full()
            .bg(to_hsla(background))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_paste))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_any_mouse_down(cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::on_mouse_up));

        view.child(TerminalElement::new(cx.entity(), self.focus_handle.clone()))
    }
}

/// IME support: committed text is sent to the program; composition text is shown at the cursor.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_text = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = None;
        if !text.is_empty() {
            self.send_input(Cow::Owned(text.as_bytes().to_vec()), cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let geometry = self.geometry?;
        Some(Bounds::new(
            geometry.cursor_origin,
            size(geometry.cell_width, geometry.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// Whether a status change is a turn ending (Working -> Idle) on a tab the user is not looking
/// at, which the sidebar shows as Done until the tab is selected.
fn finished_unseen(before: AgentStatus, after: AgentStatus, visible: bool) -> bool {
    !visible && before == AgentStatus::Working && after == AgentStatus::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_hidden_finished_turn_is_done() {
        use AgentStatus::*;
        assert!(finished_unseen(Working, Idle, false));
        assert!(!finished_unseen(Working, Idle, true));
        assert!(!finished_unseen(NeedsAttention, Idle, false));
        assert!(!finished_unseen(Working, NeedsAttention, false));
        assert!(!finished_unseen(Working, Exited, false));
    }

    #[test]
    fn default_bindings_parse() {
        assert_eq!(default_bindings().len(), 4);
    }
}
