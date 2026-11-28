//! The command palette over the workspace (`command_palette`): opening it with the rows of the
//! moment, closing it with the keyboard back where it was, and running the chosen tab,
//! action or tab menu command there.

use gpui_kit::component::WindowExt;
use gpui_kit::{Action, AppContext, Context, Entity, FocusHandle, Focusable, Subscription, Window};
use workspace_model::{PaletteState, action_label, palette_lists};

use super::{RenameOrigin, RenameTab, RenameTarget, WorkspaceView};
use crate::command_palette::{ActionRow, CommandPalette, PaletteChoice, PaletteEvent, TabRow};
use crate::keymap;
use crate::settings_file;
use crate::sidebar::TabStatus;
use crate::tab_menu::{TabCommand, TabMenuState};
use crate::terminal_view;

/// Tab menu commands the palette lists as actions already (Duplicate tab, Rename tab, Close
/// tab), so not twice.
const LISTED_AS_ACTIONS: [TabCommand; 3] =
    [TabCommand::Duplicate, TabCommand::Rename, TabCommand::Close];

/// The palette while it is open (`WorkspaceView::palette`).
pub(super) struct OpenPalette {
    view: Entity<CommandPalette>,
    /// What had the keyboard when it opened, given it back when it closes.
    return_focus: Option<FocusHandle>,
    _events: Subscription,
}

impl OpenPalette {
    pub fn view(&self) -> Entity<CommandPalette> {
        self.view.clone()
    }
}

impl WorkspaceView {
    /// Ctrl+Shift+P: open the palette, or close it while it is open. Not over a dialog (it
    /// keeps the keyboard); opening closes the branch picker and the history popup.
    pub(super) fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
            return;
        }
        if window.has_active_dialog(cx) {
            return;
        }
        self.open_palette(window, cx);
    }

    /// Open the palette over the rows of the moment, the keyboard in its field.
    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // First: closing the branch picker gives the keyboard back to what it took it from.
        self.close_popups(window, cx);
        let return_focus = window.focused(cx);
        let tabs = self.palette_tabs(cx);
        let mut actions = self.palette_actions(cx);
        actions.extend(self.palette_tab_commands(cx));
        let view = cx.new(|cx| CommandPalette::new(tabs, actions, window, cx));
        let events = cx.subscribe_in(&view, window, |workspace, _, event, window, cx| {
            workspace.on_palette_event(*event, window, cx);
        });
        self.palette = Some(OpenPalette {
            view,
            return_focus,
            _events: events,
        });
        cx.notify();
    }

    fn on_palette_event(
        &mut self,
        event: PaletteEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            PaletteEvent::Run(choice) => {
                let back = self.close_palette(window, cx);
                self.run_palette_choice(choice, back, window, cx);
            }
            PaletteEvent::Dismiss => {
                self.close_palette(window, cx);
            }
        }
    }

    /// Close the palette and, while it still has the keyboard (a click outside or a shortcut
    /// may have given it to something else), give the keyboard back to what had it. Returns
    /// that.
    fn close_palette(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        let open = self.palette.take()?;
        cx.notify();
        let palette_focused = open.view.read(cx).has_focus(window, cx);
        if palette_focused || window.focused(cx).is_none() {
            match &open.return_focus {
                Some(focus) => window.focus(focus, cx),
                None => self.focus_active(window, cx),
            }
        }
        open.return_focus
    }

    /// The other popups the palette would cover: the Git page's branch picker and the
    /// selected terminal's history popup.
    fn close_popups(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = self.git.shown_page() {
            page.update(cx, |page, cx| page.dismiss_branch_picker(window, cx));
        }
        if let Some(tab) = self.active_tab() {
            tab.view.update(cx, |view, cx| view.close_history_popup(cx));
        }
    }

    fn run_palette_choice(
        &mut self,
        choice: PaletteChoice,
        back: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match choice {
            PaletteChoice::Tab(id) => {
                if let Some(index) = self.tab_index(id) {
                    self.activate(index, window, cx);
                }
            }
            PaletteChoice::Action(action) if action == RenameTab.name() => {
                self.rename_from_palette(back, window, cx);
            }
            PaletteChoice::Action(action) => self.run_palette_action(action, window, cx),
            PaletteChoice::TabCommand(command) => {
                self.run_palette_tab_command(command, back, window, cx);
            }
        }
    }

    /// Run a tab menu command on the selected tab as its menu does. "New group with tab"
    /// then shows the tab in the sidebar, where the new group's name is edited (after the
    /// command, so the tab's old group isn't expanded); the keyboard goes back to `back`
    /// after, as for Rename tab.
    fn run_palette_tab_command(
        &mut self,
        command: TabCommand,
        back: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.active_tab().map(|tab| tab.id) else {
            return;
        };
        self.run_tab_command(id, command, window, cx);
        if command == TabCommand::NewGroup {
            self.show_in_sidebar(id, window, cx);
            self.hand_rename_to_palette(back);
        }
    }

    /// Dispatch `action` from where the keyboard is (a terminal action from the selected
    /// terminal, focused first), after this update: the workspace's own handlers can't run
    /// while it is being updated.
    fn run_palette_action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(built) = cx.build_action(action, None) else {
            return;
        };
        if is_terminal_action(action, cx)
            && let Some(tab) = self.active_tab()
        {
            window.focus(&tab.view.focus_handle(cx), cx);
        }
        window.dispatch_action(built, cx);
    }

    /// Rename tab: show the sidebar if hidden and edit the selected tab's name in its row
    /// (revealed from a collapsed group or a search); the keyboard goes back to `back` after.
    fn rename_from_palette(
        &mut self,
        back: Option<FocusHandle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.active_tab().map(|tab| tab.id) else {
            return;
        };
        self.show_in_sidebar(id, window, cx);
        self.start_rename(RenameTarget::Tab(id), RenameOrigin::Palette, window, cx);
        self.hand_rename_to_palette(back);
    }

    /// Show the sidebar if hidden and tab `id`'s row in it (revealed from a collapsed group or
    /// a search, scrolled to).
    fn show_in_sidebar(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = true;
        self.layout.reveal(id);
        self.clear_search_hiding(id, window, cx);
        self.tab_list.scroll_to_selected.set(true);
    }

    /// The name editor the palette started (or the menu's New group, run from it) gives the
    /// keyboard back to `back` when it closes.
    fn hand_rename_to_palette(&mut self, back: Option<FocusHandle>) {
        if let Some(rename) = self.renaming.as_mut() {
            rename.origin = RenameOrigin::Palette;
            rename.return_focus = back;
        }
    }

    /// The tabs in sidebar order, the selected one checked.
    fn palette_tabs(&self, cx: &Context<Self>) -> Vec<TabRow> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let view = tab.view.read(cx);
                TabRow {
                    id: tab.id,
                    name: Self::tab_name(tab, cx),
                    group: self.layout.group_of(tab.id).map(|group| group.name.clone()),
                    kind: view.kind(),
                    needs_attention: view.tab_status() == TabStatus::NeedsAttention,
                    selected: index == self.active,
                }
            })
            .collect()
    }

    /// The Keyboard page's actions that can apply now, with their first key.
    fn palette_actions(&self, cx: &Context<Self>) -> Vec<ActionRow> {
        let state = self.palette_state(cx);
        let Some(keymap) = keymap::current(cx) else {
            return Vec::new();
        };
        keymap
            .actions()
            .filter(|(action, context)| {
                palette_lists(action, *context == Some(terminal_view::KEY_CONTEXT), &state)
            })
            .map(|(action, _)| ActionRow {
                choice: PaletteChoice::Action(action),
                label: action_label(action),
                keys: keymap::hint(cx, action),
            })
            .collect()
    }

    /// The commands the selected tab's menu offers, in its order, but the ones listed as
    /// actions. None has a key.
    fn palette_tab_commands(&self, cx: &Context<Self>) -> Vec<ActionRow> {
        let Some(state) = self
            .active_tab()
            .and_then(|tab| self.tab_menu_state(tab.id, cx))
        else {
            return Vec::new();
        };
        state
            .offered()
            .into_iter()
            .filter(|command| !LISTED_AS_ACTIONS.contains(command))
            .map(|command| ActionRow {
                choice: PaletteChoice::TabCommand(command),
                label: palette_label(&state, command),
                keys: None,
            })
            .collect()
    }

    /// What decides which actions the palette lists. Reads the selected terminal's selection
    /// (one terminal lock), once per opening.
    fn palette_state(&self, cx: &Context<Self>) -> PaletteState {
        let terminal = self
            .active_tab()
            .filter(|_| !self.shows_page())
            .map(|tab| tab.view.read(cx));
        PaletteState {
            has_tab: !self.tabs.is_empty(),
            terminal_shown: terminal.is_some(),
            has_selection: terminal.is_some_and(|view| view.session.has_selection()),
            clipboard_held: terminal.is_some_and(|view| view.clipboard_pending()),
            exited: terminal.is_some_and(|view| view.has_exited()),
            file_panes_on: settings_file::file_panes(cx).panes_enabled(),
            git_page_on: self.git.enabled(),
        }
    }
}

/// A tab menu command's label in the palette: the menu's, a group named after "Move to group".
fn palette_label(state: &TabMenuState, command: TabCommand) -> String {
    let label = state.label(command);
    match command {
        TabCommand::MoveToGroup(_) => format!("Move to group: {label}"),
        _ => label.to_string(),
    }
}

/// Whether `action` is bound in the terminal's key context (it runs on the terminal).
fn is_terminal_action(action: &str, cx: &gpui_kit::App) -> bool {
    keymap::current(cx).is_some_and(|keymap| {
        keymap
            .actions()
            .any(|(name, context)| name == action && context == Some(terminal_view::KEY_CONTEXT))
    })
}

#[cfg(test)]
mod tests;
