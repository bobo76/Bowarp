use std::borrow::Cow;
use std::io;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, CursorStyle, EntityInputHandler, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, Keystroke, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoAction, ParentElement, Pixels, Point, Render,
    ScrollDelta, ScrollWheelEvent, Styled, Task, UTF16Selection, Window, div, size,
};
use term_core::alacritty_terminal::grid::Scroll;
use term_core::alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use term_core::alacritty_terminal::selection::SelectionType;
use term_core::alacritty_terminal::term::TermMode;
use term_core::{EventCallback, LaunchSpec, Palette, Session, SessionEvent, TermDimensions};

use crate::keys;
use crate::mouse::{self, Cell, MouseAction, ReportButton, WheelAction};
use crate::scrollbar::{self, ScrollbarLayout, ScrollbarState};
use crate::terminal_element::{TerminalElement, to_hsla};

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
const KEY_CONTEXT: &str = "Terminal";

/// Keys bound by the window root (focus traversal, copy) that terminal programs need: Tab for
/// completion, Shift+Tab for Claude Code's mode cycling, Ctrl+C for interrupt.
const PASSTHROUGH_KEYS: &[&str] = &["tab", "shift-tab", "ctrl-c"];

pub fn init(cx: &mut App) {
    cx.bind_keys(
        PASSTHROUGH_KEYS
            .iter()
            .map(|key| KeyBinding::new(key, NoAction, Some(KEY_CONTEXT))),
    );
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

/// One terminal session and its interaction state.
pub struct TerminalView {
    pub(crate) session: Session,
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
    pub fn new(spec: &LaunchSpec, cx: &mut Context<Self>) -> io::Result<Self> {
        let (sender, mut receiver) = mpsc::unbounded();
        let callback: EventCallback = Arc::new(move |event| {
            let _ = sender.unbounded_send(event);
        });
        let id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        let session = Session::spawn(
            id,
            spec,
            TermDimensions::new(80, 24),
            Palette::default(),
            callback,
        )?;

        let session_events = cx.spawn(async move |this, cx| {
            while let Some(event) = receiver.next().await {
                if this
                    .update(cx, |view, cx| view.on_session_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        Ok(Self {
            session,
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
        })
    }

    pub fn display_title(&self) -> String {
        let title = self.title.as_deref().unwrap_or("Agent Workspace");
        match self.process {
            ProcessState::Running => title.to_string(),
            ProcessState::Exited(Some(code)) => format!("{title} [exited {code}]"),
            ProcessState::Exited(None) => format!("{title} [exited]"),
        }
    }

    fn on_session_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        match event {
            SessionEvent::Wakeup => {}
            SessionEvent::TitleChanged(title) => self.title = title,
            SessionEvent::Bell => return,
            SessionEvent::Exited(code) => self.process = ProcessState::Exited(code),
        }
        cx.notify();
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

    fn copy(&mut self, cx: &mut Context<Self>) {
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
        let consumed = self.handle_clipboard_shortcut(&event.keystroke, cx)
            || self.handle_scrollback_shortcut(&event.keystroke, cx)
            || self.forward_key(event, cx);
        if consumed {
            cx.stop_propagation();
        }
    }

    /// Ctrl+Shift+C copies; Ctrl+Shift+V and Shift+Insert paste.
    fn handle_clipboard_shortcut(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = &keystroke.modifiers;
        let control_shift = modifiers.control && modifiers.shift && !modifiers.alt;
        match keystroke.key.as_str() {
            "c" if control_shift => self.copy(cx),
            "v" if control_shift => self.paste_clipboard(cx),
            "insert" if is_shift_only(modifiers) => self.paste_clipboard(cx),
            _ => return false,
        }
        true
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

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
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

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.display_title();
        if window.window_title() != title {
            window.set_window_title(&title);
        }

        let background = self.session.palette().background;
        let view = div()
            .size_full()
            .bg(to_hsla(background))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
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
