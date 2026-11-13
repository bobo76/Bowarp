use gpui_kit::Focusable;
use workspace_model::FeatureSwitch;

use super::super::ToggleGitPage;
use super::super::tests::PanesHarness;
use crate::settings_file;
use crate::shortcuts::{self, GIT_PAGE_KEYS};

/// Checks (Git page shows, settings page shows, the selected tab shows, Git row drawn,
/// Settings row drawn). `debug_bounds` needs debug assertions: a release test run skips the
/// two drawn rows.
#[track_caller]
fn assert_pages(h: &mut PanesHarness, expected: (bool, bool, bool, bool, bool)) {
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    let shown = h.cx.update(|_, cx| {
        let workspace = workspace.read(cx);
        (
            workspace.git.shows_page(),
            workspace.settings.shows_page(),
            workspace.current_index().is_some(),
        )
    });
    assert_eq!(shown, (expected.0, expected.1, expected.2));
    if cfg!(debug_assertions) {
        let rows = (
            h.cx.debug_bounds("git-row").is_some(),
            h.cx.debug_bounds("settings-row").is_some(),
        );
        assert_eq!(rows, (expected.3, expected.4));
    }
}

#[gpui::test]
fn git_page_replaces_the_tab_and_closing_gives_it_back(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("git-page-toggle", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_git(window, cx)));
    h.cx.run_until_parked();
    assert_pages(&mut h, (true, false, false, true, false));
    h.cx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(workspace.shows_page());
        assert!(workspace.git.focus(cx).unwrap().is_focused(window));
    });

    // The button again, with the page focused, closes it.
    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.toggle_git(window, cx)));
    h.cx.run_until_parked();
    assert_pages(&mut h, (false, false, true, false, false));
    h.cx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(workspace.tabs[0].view.focus_handle(cx).is_focused(window));
    });
}

#[gpui::test]
fn git_and_settings_pages_take_turns(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("git-page-settings", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_git(window, cx);
            w.open_settings(window, cx);
        })
    });
    h.cx.run_until_parked();
    assert_pages(&mut h, (false, true, false, true, true));

    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_git(window, cx)));
    h.cx.run_until_parked();
    assert_pages(&mut h, (true, false, false, true, true));
    h.cx.update(|window, cx| {
        assert!(workspace.read(cx).git.focus(cx).unwrap().is_focused(window));
    });

    // Selecting the tab hides the page; both rows stay.
    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.activate(0, window, cx)));
    h.cx.run_until_parked();
    assert_pages(&mut h, (false, false, true, true, true));
}

#[gpui::test]
fn closing_the_git_page_without_tabs_shows_the_empty_state(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("git-page-no-tab", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.open_git(window, cx);
            w.close_tab(0, window, cx);
        })
    });
    h.cx.run_until_parked();
    h.cx.update(|_, cx| {
        let workspace = workspace.read(cx);
        assert!(workspace.tabs.is_empty() && workspace.git.shows_page());
        assert!(!workspace.explorer_shown());
    });

    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.close_active(window, cx)));
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.run_until_parked();
    h.cx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(!workspace.shows_page() && !workspace.git.open);
        assert!(workspace.empty_focus.is_focused(window));
    });
}

fn set_git_page(h: &mut PanesHarness, on: bool) {
    h.cx.update(|_, cx| {
        settings_file::update(cx, |settings| settings.git_page = FeatureSwitch::new(on));
    });
    h.cx.run_until_parked();
}

fn press_git_key(h: &mut PanesHarness) {
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    h.cx.simulate_keystrokes(GIT_PAGE_KEYS);
    h.cx.run_until_parked();
}

fn terminal_focused(h: &mut PanesHarness) -> bool {
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| {
        workspace.read(cx).tabs[0]
            .view
            .focus_handle(cx)
            .is_focused(window)
    })
}

#[gpui::test]
fn the_git_key_opens_the_page_and_goes_back_to_the_tab(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("git-page-key", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.focus_active(window, cx)));
    assert!(terminal_focused(&mut h));

    // From the terminal: the key is not sent to it, the page opens with the keyboard.
    press_git_key(&mut h);
    assert_pages(&mut h, (true, false, false, true, false));
    h.cx.update(|window, cx| {
        assert!(workspace.read(cx).git.focus(cx).unwrap().is_focused(window));
    });

    // Pressed again: back to the tab, which gets the keyboard; the row goes.
    press_git_key(&mut h);
    assert_pages(&mut h, (false, false, true, false, false));
    assert!(terminal_focused(&mut h));
}

#[gpui::test]
fn the_git_key_goes_back_even_when_the_page_lost_the_keyboard(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("git-page-key-unfocused", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_git(window, cx)));
    h.cx.run_until_parked();
    // The tab list takes the keyboard; the page still shows.
    h.cx.update(|window, cx| {
        let list = workspace.read(cx).tab_list.focus.clone();
        window.focus(&list, cx);
    });
    press_git_key(&mut h);
    assert_pages(&mut h, (false, false, true, false, false));
}

#[gpui::test]
fn the_git_key_does_nothing_while_the_switch_is_off(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("git-page-key-off", cx);
    let workspace = h.workspace.clone();
    set_git_page(&mut h, false);
    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.focus_active(window, cx)));
    press_git_key(&mut h);
    assert_pages(&mut h, (false, false, true, false, false));
    assert!(terminal_focused(&mut h));
    // The button's method and the row's do nothing either.
    h.cx.update(|window, cx| {
        workspace.update(cx, |w, cx| {
            w.toggle_git(window, cx);
            w.show_git(None, window, cx);
        })
    });
    assert_pages(&mut h, (false, false, true, false, false));
    assert!(h.cx.update(|_, cx| workspace.read(cx).git.page.is_none()));
}

#[gpui::test]
fn switching_off_closes_the_page_and_keeps_it_for_later(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("git-page-switch-off", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_git(window, cx)));
    h.cx.run_until_parked();
    let page =
        h.cx.update(|_, cx| workspace.read(cx).git.page.clone().unwrap());
    // The title bar draws the button while `enabled` (Buttons record no debug bounds).
    let button = |h: &mut PanesHarness| h.cx.update(|_, cx| workspace.read(cx).git.enabled());
    assert!(button(&mut h));

    set_git_page(&mut h, false);
    assert_pages(&mut h, (false, false, true, false, false));
    assert!(terminal_focused(&mut h));
    h.cx.update(|_, cx| {
        let kept = workspace.read(cx).git.page.clone().unwrap();
        assert_eq!(kept, page, "the page and its commit drafts stay");
    });
    assert!(!button(&mut h));

    // Back on: the button returns, the row only once the page opens.
    set_git_page(&mut h, true);
    assert_pages(&mut h, (false, false, true, false, false));
    assert!(button(&mut h));
}

#[cfg(not(target_os = "macos"))]
#[gpui::test]
fn the_git_button_tooltip_names_the_key(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("git-page-tooltip", cx);
    let tooltip =
        h.cx.update(|_, cx| shortcuts::with_shortcut("Git", &ToggleGitPage, cx));
    assert_eq!(tooltip.as_ref(), "Git (Ctrl+Shift+G)");
}
