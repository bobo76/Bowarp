use super::*;

use gpui_kit::component::WindowExt;

use crate::workspace_view::tests::{PanesHarness, wait_for_exit};
use crate::workspace_view::{SearchTabs, ToggleCommandPalette, ToggleSidebar};

/// Three tabs named one, two, three; the first selected with the keyboard on its terminal.
fn harness(name: &str, cx: &mut gpui_kit::TestAppContext) -> (PanesHarness, Vec<u64>) {
    let mut h = PanesHarness::new(name, cx);
    let workspace = h.workspace.clone();
    let ids = h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            for _ in 0..2 {
                let spec = workspace.tabs[0].spec.clone();
                workspace.open_tab_or_notify(spec, window, cx);
                wait_for_exit(&workspace.active_tab().unwrap().view.read(cx).session);
            }
            for (tab, name) in workspace.tabs.iter_mut().zip(["one", "two", "three"]) {
                tab.meta.custom_name = Some(name.into());
            }
            workspace.activate(0, window, cx);
            workspace.tabs.iter().map(|tab| tab.id).collect()
        })
    });
    h.cx.run_until_parked();
    (h, ids)
}

/// Draw, then press `keys` (bindings and focus come from the last drawn frame).
fn press(h: &mut PanesHarness, keys: &str) {
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.simulate_keystrokes(keys);
    h.cx.run_until_parked();
}

fn type_text(h: &mut PanesHarness, text: &str) {
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.simulate_input(text);
    h.cx.run_until_parked();
}

fn palette(h: &mut PanesHarness) -> Option<Entity<CommandPalette>> {
    h.workspace.read_with(&h.cx, |workspace, _| {
        workspace.palette.as_ref().map(OpenPalette::view)
    })
}

fn palette_focused(h: &mut PanesHarness) -> bool {
    let Some(palette) = palette(h) else {
        return false;
    };
    h.cx.update(|window, cx| palette.read(cx).has_focus(window, cx))
}

fn shown(h: &mut PanesHarness) -> Vec<PaletteChoice> {
    let palette = palette(h).expect("the palette is open");
    h.cx.update(|_, cx| palette.read(cx).shown_choices(cx))
}

fn selected_tab(h: &mut PanesHarness) -> u64 {
    h.workspace
        .read_with(&h.cx, |workspace, _| workspace.active_tab().unwrap().id)
}

fn terminal_focused(h: &mut PanesHarness) -> bool {
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| {
        let tab = workspace.read(cx).active_tab().unwrap().view.clone();
        tab.focus_handle(cx).is_focused(window)
    })
}

fn has_action(choices: &[PaletteChoice], action: &dyn Action) -> bool {
    choices.contains(&PaletteChoice::Action(action.name()))
}

#[gpui::test]
fn the_key_opens_the_palette_and_closes_it_with_the_keyboard_back(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (mut h, ids) = harness("palette-toggle", cx);
    assert!(terminal_focused(&mut h));
    press(&mut h, "ctrl-shift-p");
    assert!(palette_focused(&mut h), "the field has the keyboard");
    // Tabs in sidebar order, then the actions; the palette's own key is not listed.
    let choices = shown(&mut h);
    assert_eq!(
        choices[..3],
        ids.iter()
            .map(|&id| PaletteChoice::Tab(id))
            .collect::<Vec<_>>()[..]
    );
    assert!(has_action(&choices, &ToggleSidebar));
    assert!(!has_action(&choices, &ToggleCommandPalette));
    // Tab keeps the keyboard in the palette.
    press(&mut h, "tab");
    assert!(palette_focused(&mut h));
    press(&mut h, "ctrl-shift-p");
    assert!(palette(&mut h).is_none());
    assert!(terminal_focused(&mut h));
    press(&mut h, "ctrl-shift-p");
    press(&mut h, "escape");
    assert!(palette(&mut h).is_none(), "Esc closes it");
    assert!(terminal_focused(&mut h));
}

#[gpui::test]
fn typing_ranks_the_rows_and_enter_goes_to_the_best_tab(cx: &mut gpui_kit::TestAppContext) {
    let (mut h, ids) = harness("palette-tab", cx);
    press(&mut h, "ctrl-shift-p");
    type_text(&mut h, "thr");
    let palette_view = palette(&mut h).unwrap();
    let selected =
        h.cx.update(|_, cx| palette_view.read(cx).selected_choice(cx));
    assert_eq!(selected, Some(PaletteChoice::Tab(ids[2])));
    press(&mut h, "enter");
    assert!(palette(&mut h).is_none());
    assert_eq!(selected_tab(&mut h), ids[2]);
    assert!(
        terminal_focused(&mut h),
        "the chosen tab's terminal has the keyboard"
    );
    // The query starts empty again.
    press(&mut h, "ctrl-shift-p");
    assert_eq!(shown(&mut h).len(), shown(&mut h).len());
    assert_eq!(shown(&mut h)[0], PaletteChoice::Tab(ids[0]));
}

#[gpui::test]
fn enter_runs_a_workspace_action_from_where_the_keyboard_was(cx: &mut gpui_kit::TestAppContext) {
    let (mut h, _) = harness("palette-action", cx);
    let sidebar = |h: &mut PanesHarness| h.workspace.read_with(&h.cx, |w, _| w.sidebar_visible);
    assert!(sidebar(&mut h));
    press(&mut h, "ctrl-shift-p");
    type_text(&mut h, "hide sidebar");
    assert_eq!(shown(&mut h), [PaletteChoice::Action(ToggleSidebar.name())]);
    press(&mut h, "enter");
    assert!(palette(&mut h).is_none());
    assert!(!sidebar(&mut h));
    assert!(terminal_focused(&mut h));
    // An action that moves the keyboard (search tabs) takes it from there.
    press(&mut h, "ctrl-shift-p");
    type_text(&mut h, "searchtabs");
    assert_eq!(shown(&mut h), [PaletteChoice::Action(SearchTabs.name())]);
    press(&mut h, "enter");
    assert!(sidebar(&mut h), "search shows the sidebar");
    let search_focused = h.cx.update(|window, cx| {
        let workspace = h.workspace.read(cx);
        workspace.search.has_focus(window, cx)
    });
    assert!(search_focused);
}

#[gpui::test]
fn a_terminal_action_runs_on_the_selected_terminal(cx: &mut gpui_kit::TestAppContext) {
    let (mut h, _) = harness("palette-terminal", cx);
    // Opened from the sidebar's search field, the action still reaches the terminal.
    h.cx.update(|window, cx| {
        h.workspace
            .update(cx, |workspace, cx| workspace.focus_search(window, cx));
    });
    press(&mut h, "ctrl-shift-p");
    type_text(&mut h, "select all");
    press(&mut h, "enter");
    assert!(terminal_focused(&mut h));
    let selection = h.cx.update(|_, cx| {
        let tab = h.workspace.read(cx).active_tab().unwrap().view.clone();
        tab.read(cx).session.has_selection()
    });
    assert!(selection);
}

#[gpui::test]
fn actions_that_cannot_apply_are_hidden(cx: &mut gpui_kit::TestAppContext) {
    use crate::terminal_view::{AllowClipboard, Copy, CopyOrSend, Find, Paste};
    use crate::workspace_view::{SelectTab1, ToggleExplorer, ToggleGitPage};

    let (mut h, _) = harness("palette-hidden", cx);
    h.set_panes(false);
    press(&mut h, "ctrl-shift-p");
    let choices = shown(&mut h);
    // No selection, no held write, an exited tab (the test tabs' program exits at once).
    for action in [
        &Copy as &dyn Action,
        &CopyOrSend,
        &Paste,
        &AllowClipboard,
        &SelectTab1,
        &ToggleExplorer,
    ] {
        assert!(!has_action(&choices, action), "{}", action.name());
    }
    assert!(has_action(&choices, &Find));
    let git_on = h
        .workspace
        .read_with(&h.cx, |workspace, _| workspace.git.enabled());
    assert_eq!(has_action(&choices, &ToggleGitPage), git_on);
    press(&mut h, "escape");
    h.set_panes(true);
    press(&mut h, "ctrl-shift-p");
    assert!(has_action(&shown(&mut h), &ToggleExplorer));
    press(&mut h, "escape");
    // On the Settings page, the terminal's actions are gone.
    h.cx.update(|window, cx| {
        h.workspace
            .update(cx, |workspace, cx| workspace.open_settings(window, cx));
    });
    press(&mut h, "ctrl-shift-p");
    assert!(!has_action(&shown(&mut h), &Find));
}

#[gpui::test]
fn page_keys_move_by_a_page(cx: &mut gpui_kit::TestAppContext) {
    let (mut h, ids) = harness("palette-page", cx);
    press(&mut h, "ctrl-shift-p");
    let choices = shown(&mut h);
    let selected = |h: &mut PanesHarness| {
        let palette = palette(h).unwrap();
        h.cx.update(|_, cx| palette.read(cx).selected_choice(cx))
    };
    assert_eq!(selected(&mut h), Some(PaletteChoice::Tab(ids[0])));
    press(&mut h, "pagedown");
    assert_eq!(selected(&mut h), Some(choices[10]));
    press(&mut h, "down");
    assert_eq!(selected(&mut h), Some(choices[11]));
    press(&mut h, "pageup");
    assert_eq!(selected(&mut h), Some(choices[1]));
    assert!(palette_focused(&mut h));
}

#[gpui::test]
fn the_keyboard_leaving_closes_it_and_a_dialog_keeps_it_closed(cx: &mut gpui_kit::TestAppContext) {
    let (mut h, _) = harness("palette-focus", cx);
    h.cx.update(|window, _| window.activate_window());
    press(&mut h, "ctrl-shift-p");
    // Another app taking the keyboard leaves it open, the keyboard in it on the way back.
    h.cx.deactivate_window();
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.run_until_parked();
    assert!(palette(&mut h).is_some());
    h.cx.update(|window, _| window.activate_window());
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.run_until_parked();
    assert!(palette_focused(&mut h));
    // A workspace key that moves the keyboard: the palette goes, the keyboard stays there.
    press(&mut h, "ctrl-shift-s");
    // Focus listeners run on the next frame, and only see an active window's focus.
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.run_until_parked();
    assert!(palette(&mut h).is_none());
    let search_focused = h.cx.update(|window, cx| {
        let workspace = h.workspace.read(cx);
        workspace.search.has_focus(window, cx)
    });
    assert!(search_focused);
    // It doesn't open over a dialog.
    h.cx.update(|window, cx| {
        window.open_alert_dialog(cx, |alert, _, _| alert.title("Other").show_cancel(true));
    });
    h.cx.run_until_parked();
    h.cx.update(|window, cx| {
        h.workspace.update(cx, |workspace, cx| {
            workspace.toggle_command_palette(window, cx)
        });
    });
    assert!(palette(&mut h).is_none());
}

#[gpui::test]
fn a_dialog_opened_by_a_key_leaves_the_palette_open_and_gives_the_keyboard_back(
    cx: &mut gpui_kit::TestAppContext,
) {
    use term_core::SessionEvent;

    let (mut h, ids) = harness("palette-dialog", cx);
    h.cx.update(|window, _| window.activate_window());
    // The selected tab runs a command, so closing it asks first.
    h.cx.update(|_, cx| {
        h.workspace.update(cx, |workspace, cx| {
            workspace.tabs[0].view.update(cx, |view, cx| {
                view.revive_for_test();
                view.dispatch_test_event(SessionEvent::CommandStarted(Some("sleep 9".into())), cx);
            });
        })
    });
    press(&mut h, "ctrl-shift-p");
    press(&mut h, "ctrl-shift-w");
    let dialog = |h: &mut PanesHarness| h.cx.update(|window, cx| window.has_active_dialog(cx));
    assert!(dialog(&mut h));
    // Focus listeners run on the next frame.
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.run_until_parked();
    assert!(palette(&mut h).is_some());
    press(&mut h, "escape");
    assert!(!dialog(&mut h));
    assert!(palette(&mut h).is_some());
    assert!(palette_focused(&mut h));
    // The keyboard is in the field again.
    type_text(&mut h, "two");
    assert_eq!(
        shown(&mut h).first().copied(),
        Some(PaletteChoice::Tab(ids[1]))
    );
}

#[gpui::test]
fn rename_tab_shows_the_sidebar_and_gives_the_keyboard_back(cx: &mut gpui_kit::TestAppContext) {
    let (mut h, ids) = harness("palette-rename", cx);
    h.cx.update(|window, cx| {
        h.workspace
            .update(cx, |workspace, cx| workspace.toggle_sidebar(window, cx));
    });
    press(&mut h, "ctrl-shift-p");
    type_text(&mut h, "rename");
    press(&mut h, "enter");
    let (visible, target) = h.workspace.read_with(&h.cx, |workspace, _| {
        let target = workspace.renaming.as_ref().map(|rename| rename.target);
        (workspace.sidebar_visible, target)
    });
    assert!(visible);
    assert_eq!(target, Some(RenameTarget::Tab(ids[0])));
    press(&mut h, "escape");
    let renaming = h
        .workspace
        .read_with(&h.cx, |workspace, _| workspace.renaming.is_some());
    assert!(!renaming);
    assert!(terminal_focused(&mut h));
}
