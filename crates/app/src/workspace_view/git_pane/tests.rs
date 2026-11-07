use gpui_kit::Focusable;

use super::super::tests::PanesHarness;

/// (Git page shows, settings page shows, the selected tab shows, Git row drawn, Settings row
/// drawn).
fn pages(h: &mut PanesHarness) -> (bool, bool, bool, bool, bool) {
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| window.draw(cx).clear(cx));
    let rows = (
        h.cx.debug_bounds("git-row").is_some(),
        h.cx.debug_bounds("settings-row").is_some(),
    );
    h.cx.update(|_, cx| {
        let workspace = workspace.read(cx);
        (
            workspace.git.shows_page(),
            workspace.settings.shows_page(),
            workspace.current_index().is_some(),
            rows.0,
            rows.1,
        )
    })
}

#[gpui::test]
fn git_page_replaces_the_tab_and_closing_gives_it_back(cx: &mut gpui_kit::TestAppContext) {
    let mut h = PanesHarness::new("git-page-toggle", cx);
    let workspace = h.workspace.clone();
    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_git(window, cx)));
    h.cx.run_until_parked();
    assert_eq!(pages(&mut h), (true, false, false, true, false));
    h.cx.update(|window, cx| {
        let workspace = workspace.read(cx);
        assert!(workspace.shows_page());
        assert!(workspace.git.focus(cx).unwrap().is_focused(window));
    });

    // The button again, with the page focused, closes it.
    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.toggle_git(window, cx)));
    h.cx.run_until_parked();
    assert_eq!(pages(&mut h), (false, false, true, false, false));
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
    assert_eq!(pages(&mut h), (false, true, false, true, true));

    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_git(window, cx)));
    h.cx.run_until_parked();
    assert_eq!(pages(&mut h), (true, false, false, true, true));
    h.cx.update(|window, cx| {
        assert!(workspace.read(cx).git.focus(cx).unwrap().is_focused(window));
    });

    // Selecting the tab hides the page; both rows stay.
    h.cx.update(|window, cx| workspace.update(cx, |w, cx| w.activate(0, window, cx)));
    h.cx.run_until_parked();
    assert_eq!(pages(&mut h), (false, false, true, true, true));
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
