use std::borrow::Cow;
use std::io;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, CursorStyle, EntityInputHandler, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoAction, ParentElement, Pixels, Point, Render,
    ScrollDelta, ScrollWheelEvent, Styled, Task, UTF16Selection, Window, div, px, size,
};
use term_core::alacritty_terminal::grid::Scroll;
use term_core::alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use term_core::alacritty_terminal::selection::{Selection, SelectionType};
use term_core::alacritty_terminal::term::TermMode;
use term_core::{EventCallback, LaunchSpec, Palette, Session, SessionEvent, TermDimensions};

use crate::keys;
use crate::mouse::{self, Cell, MouseAction, ReportButton, WheelAction};
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

/// One terminal session and its interaction state.
pub struct TerminalView {
    pub(crate) session: Session,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) geometry: Option<GridGeometry>,
    /// IME composition text not yet committed.
    pub(crate) marked_text: Option<String>,
    title: Option<String>,
    exit: Option<Option<i32>>,
    scroll_remainder: f32,
    selecting: bool,
    /// Button whose press was reported to the program; its release and drags are reported too.
    reported_button: Option<ReportButton>,
    /// Last cell a motion report was sent for, so motion is reported once per cell.
    last_motion_cell: Option<Cell>,
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
            exit: None,
            scroll_remainder: 0.0,
            selecting: false,
            reported_button: None,
            last_motion_cell: None,
            _session_events: session_events,
        })
    }

    pub fn display_title(&self) -> String {
        let title = self.title.as_deref().unwrap_or("Agent Workspace");
        match self.exit {
            None => title.to_string(),
            Some(Some(code)) => format!("{title} [exited {code}]"),
            Some(None) => format!("{title} [exited]"),
        }
    }

    fn on_session_event(&mut self, event: SessionEvent, cx: &mut Context<Self>) {
        match event {
            SessionEvent::Wakeup => {}
            SessionEvent::TitleChanged(title) => self.title = title,
            SessionEvent::Bell => return,
            SessionEvent::Exited(code) => self.exit = Some(code),
        }
        cx.notify();
    }

    /// Send user input to the program, snapping the view back to the live screen.
    fn send_input(&mut self, bytes: Cow<'static, [u8]>, cx: &mut Context<Self>) {
        {
            let mut term = self.session.lock_term();
            term.scroll_display(Scroll::Bottom);
            term.selection = None;
        }
        self.session.write(bytes);
        cx.notify();
    }

    fn mode(&self) -> TermMode {
        *self.session.lock_term().mode()
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let text = self.session.lock_term().selection_to_string();
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
        let keystroke = &event.keystroke;
        let modifiers = &keystroke.modifiers;
        let key = keystroke.key.as_str();

        if modifiers.control && modifiers.shift && !modifiers.alt {
            match key {
                "c" => self.copy(cx),
                "v" => self.paste(cx.read_from_clipboard().and_then(|item| item.text()), cx),
                _ => {}
            }
            if matches!(key, "c" | "v") {
                cx.stop_propagation();
                return;
            }
        }
        if modifiers.shift && !modifiers.control && !modifiers.alt {
            let scroll = match key {
                "insert" => {
                    self.paste(cx.read_from_clipboard().and_then(|item| item.text()), cx);
                    cx.stop_propagation();
                    return;
                }
                "pageup" => Some(Scroll::PageUp),
                "pagedown" => Some(Scroll::PageDown),
                _ => None,
            };
            if let Some(scroll) = scroll {
                self.session.lock_term().scroll_display(scroll);
                cx.notify();
                cx.stop_propagation();
                return;
            }
        }

        let Some(bytes) = keys::key_to_bytes(keystroke, event.prefer_character_input, self.mode())
        else {
            return;
        };
        self.send_input(bytes, cx);
        cx.stop_propagation();
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
            WheelAction::Scrollback(lines) => {
                self.session
                    .lock_term()
                    .scroll_display(Scroll::Delta(lines));
            }
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
        let display_offset = self.session.lock_term().grid().display_offset();
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
        if mouse::reporting(self.mode(), &event.modifiers) {
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
        self.session.lock_term().selection = Some(Selection::new(selection_type, point, side));
        self.selecting = true;
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.reported_button.is_some() || mouse::reporting(self.mode(), &event.modifiers) {
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
        if let Some(selection) = self.session.lock_term().selection.as_mut() {
            selection.update(point, side);
        }
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
        let text = {
            let mut term = self.session.lock_term();
            if term.selection.as_ref().is_some_and(Selection::is_empty) {
                term.selection = None;
            }
            term.selection_to_string()
        };
        #[cfg(target_os = "linux")]
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = text;
        cx.notify();
    }
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
            .px(px(8.))
            .py(px(4.))
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
