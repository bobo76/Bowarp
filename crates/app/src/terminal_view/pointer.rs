//! The mouse over the grid: wheel scrolling, text selection, and reports to programs that
//! track the mouse.

#[cfg(target_os = "linux")]
use gpui_kit::ClipboardItem;
use gpui_kit::{
    Context, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollDelta, ScrollWheelEvent, Window,
};
use term_core::alacritty_terminal::grid::Scroll;
use term_core::alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use term_core::alacritty_terminal::selection::SelectionType;
use workspace_model::{WheelScroll, notch_lines};

use super::TerminalView;
use crate::mouse::{self, Cell, MouseAction, ReportButton, WheelAction};

#[derive(Default)]
pub(super) struct PointerState {
    wheel: WheelScroll,
    selecting: bool,
    /// Button whose press was reported to the program; its release and drags are reported too.
    reported_button: Option<ReportButton>,
    /// Last cell a motion report was sent for, so motion is reported once per cell.
    last_motion_cell: Option<Cell>,
}

impl TerminalView {
    pub(super) fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(geometry) = self.geometry else {
            return;
        };
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => notch_lines(delta.y, cfg!(target_os = "linux")),
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / f32::from(geometry.line_height),
        };
        let whole = self.pointer.wheel.add(lines);
        if whole == 0 {
            return;
        }

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
            if self.pointer.last_motion_cell == Some(cell) {
                return;
            }
            self.pointer.last_motion_cell = Some(cell);
        }
        if let Some(bytes) = mouse::report(button, action, cell, modifiers, self.mode()) {
            self.session.write(bytes);
        }
    }

    pub(super) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.close_history_popup(cx);
        // A link takes the click before the program's mouse reports and the selection.
        if self.open_clicked_link(event, cx) {
            return;
        }
        let Some(button) = ReportButton::from_gpui(event.button) else {
            return;
        };
        if mouse::is_reporting_to_program(self.mode(), &event.modifiers) {
            self.send_mouse_report(button, MouseAction::Press, event.position, &event.modifiers);
            self.pointer.reported_button = Some(button);
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
        self.pointer.selecting = true;
        cx.notify();
    }

    pub(super) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pointer.reported_button.is_some()
            || mouse::is_reporting_to_program(self.mode(), &event.modifiers)
        {
            let button = self.pointer.reported_button.unwrap_or(ReportButton::None);
            self.send_mouse_report(
                button,
                MouseAction::Motion,
                event.position,
                &event.modifiers,
            );
            return;
        }
        if !self.pointer.selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some((point, side)) = self.grid_point(event.position) else {
            return;
        };
        self.session.update_selection(point, side);
        cx.notify();
    }

    pub(super) fn on_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let button = ReportButton::from_gpui(event.button);
        if button.is_some() && button == self.pointer.reported_button {
            self.pointer.reported_button = None;
            self.send_mouse_report(
                button.unwrap_or(ReportButton::None),
                MouseAction::Release,
                event.position,
                &event.modifiers,
            );
            return;
        }
        if !self.pointer.selecting || event.button != MouseButton::Left {
            return;
        }
        self.pointer.selecting = false;
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
