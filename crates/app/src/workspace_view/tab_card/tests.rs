use super::*;

#[gpui::test]
fn card_settings_stop_existing_clocks_and_skip_disabled_work(cx: &mut gpui_kit::TestAppContext) {
    use crate::terminal_view::{TabChanged, TerminalView};
    use gpui_kit::VisualTestContext;
    use gpui_kit::component::Root;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, mpsc};
    use term_core::{LaunchSpec, Palette, Session, SessionEvent, SessionKind, TermDimensions};

    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init(cx);
        crate::terminal_element::init(cx);
        settings_file::preview(cx, |settings| settings.tab_card = Default::default());
    });
    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let spec = LaunchSpec::new(
        SessionKind::Shell,
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        Some(std::env::current_dir().unwrap()),
    );
    let (exited, exits) = mpsc::channel();
    let session = Session::spawn(
        1,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(move |event| {
            if matches!(event, SessionEvent::Exited(_)) {
                let _ = exited.send(());
            }
        }),
    )
    .unwrap();
    exits.recv_timeout(Duration::from_secs(30)).unwrap();
    let captured = Rc::new(RefCell::new(None));
    let capture = captured.clone();
    let window = cx.add_window(move |window, cx| {
        let (_, messages) = futures::channel::mpsc::unbounded();
        let workspace = cx.new(|cx| {
            WorkspaceView::new(
                Rc::from([]),
                None,
                messages,
                Rc::new(crate::tabs_file::TabsWriter::new(false)),
                window,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| {
            let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
            let events = std::array::from_fn(|_| cx.subscribe(&view, |_, _, _: &TabChanged, _| {}));
            workspace.tabs.push(super::super::Tab {
                id: 1,
                view,
                spec,
                meta: workspace_model::TabMeta {
                    custom_name: Some("Custom tab name".into()),
                    ..Default::default()
                },
                viewer: None,
                _view_events: events,
            });
            workspace.layout.add(1);
            let card = workspace.tab_card(1, cx).unwrap();
            assert_eq!(card.name.as_ref(), "Custom tab name");
            assert!(card.title.is_some());
            assert!(card.status.is_some());
            assert!(card.kind.is_some());
            assert!(card.folder.is_some());
            workspace.cards.shown = Some(1);
            workspace.cards.clock = Some((
                1,
                cx.spawn(async |_, _| {
                    futures::future::pending::<()>().await;
                }),
            ));
        });
        *capture.borrow_mut() = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let workspace = captured.borrow().clone().unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|_, cx| {
        settings_file::preview(cx, |settings| {
            settings.tab_card.status = false;
            settings.tab_card.changes = false;
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            assert!(workspace.cards.clock.is_none());
            workspace.start_card_clock(1, cx);
            workspace.refresh_diff_stat(1, cx);
            assert!(workspace.cards.clock.is_none());
            assert!(workspace.cards.diff_stats.is_empty());
            let card = workspace.tab_card(1, cx).unwrap();
            assert!(card.status.is_none());
            assert!(card.working_for.is_none());
            assert!(card.running_for.is_none());
            assert!(card.running_label.is_none());
            assert!(card.lines_added.is_none());
            assert!(card.title.is_some());
            assert!(card.kind.is_some());
        })
    });
    cx.update(|_, cx| {
        settings_file::preview(cx, |settings| {
            settings.tab_card.location = false;
            settings.tab_card.program_title = false;
            settings.tab_card.session_kind = false;
        })
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            let fields = settings_file::settings(cx).tab_card;
            assert!(fields.enabled);
            assert!(!fields.active());
            assert!(workspace.tab_card(1, cx).is_none());
            assert!(workspace.cards.shown.is_none());
            workspace.start_card_clock(1, cx);
            workspace.refresh_diff_stat(1, cx);
            assert!(workspace.cards.clock.is_none());
            assert!(workspace.cards.diff_stats.is_empty());
        })
    });
    cx.update(|_, cx| settings_file::preview(cx, |settings| settings.tab_card.enabled = false));
    cx.run_until_parked();
    cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            assert!(workspace.cards.shown.is_none());
            assert!(workspace.tab_card(1, cx).is_none());
        })
    });
}

#[test]
fn diff_stat_is_reused_for_the_same_folder_only_briefly() {
    let repo = Path::new("/repo");
    let mut stat = DiffStat::new(repo.to_path_buf());
    let now = Instant::now();
    assert!(stat.needs_run(repo, now));
    let generation = stat.start(repo);
    assert!(stat.finish(generation, Some(3), now));

    assert!(!stat.needs_run(repo, now + Duration::from_secs(1)));
    assert!(stat.needs_run(repo, now + DIFF_STAT_TTL));
    assert!(stat.needs_run(Path::new("/other"), now));
}

#[test]
fn diff_stat_runs_once_at_a_time_per_folder() {
    let repo = Path::new("/repo");
    let mut stat = DiffStat::new(repo.to_path_buf());
    let now = Instant::now();
    let generation = stat.start(repo);
    // However long git takes, opening the card again does not start another one.
    assert!(!stat.needs_run(repo, now + DIFF_STAT_TTL * 10));
    assert!(stat.finish(generation, Some(3), now));
    // A later run keeps showing the last answer until it has its own.
    stat.start(repo);
    assert_eq!(stat.added, Some(3));
}

#[test]
fn diff_stat_ignores_an_answer_for_a_folder_left_meanwhile() {
    let mut stat = DiffStat::new(PathBuf::from("/repo"));
    let now = Instant::now();
    let old = stat.start(Path::new("/repo"));
    let other = Path::new("/other");
    assert!(stat.needs_run(other, now));
    let new = stat.start(other);
    assert_eq!(stat.added, None);

    assert!(!stat.finish(old, Some(3), now));
    assert_eq!(stat.added, None);
    assert!(stat.running);
    assert!(stat.finish(new, Some(7), now));
    assert_eq!((stat.folder.as_path(), stat.added), (other, Some(7)));
}

#[test]
fn cards_show_lines_added_only_for_the_folder_they_were_counted_in() {
    let mut cards = TabCards::default();
    let (repo, now) = (Path::new("/repo"), Instant::now());
    let generation = cards.start_diff_stat(1, repo, now).unwrap();
    assert_eq!(cards.start_diff_stat(1, repo, now), None);
    assert!(cards.finish_diff_stat(1, generation, Some(4), now));

    assert_eq!(cards.lines_added(1, repo), Some(4));
    assert_eq!(cards.lines_added(1, Path::new("/other")), None);
    cards.forget(1);
    assert_eq!(cards.lines_added(1, repo), None);
}

#[gpui::test]
fn cards_are_held_while_a_name_editor_a_menu_or_a_dialog_is_open(
    cx: &mut gpui_kit::TestAppContext,
) {
    use super::super::RenameTarget;
    use super::super::tests::PanesHarness;
    use gpui_kit::component::menu::PopupMenu;
    use gpui_kit::{DismissEvent, Entity};

    let mut h = PanesHarness::new("held-cards", cx);
    // No `git diff` run in a test.
    h.cx.update(|_, cx| settings_file::preview(cx, |settings| settings.tab_card.changes = false));
    let workspace = h.workspace.clone();
    let id = workspace.read_with(&h.cx, |workspace, _| workspace.tabs[0].id);
    // The tab's card shows, as its hover card reports when it opens.
    h.cx.update(|_, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.cards.shown = Some(id);
            workspace.start_card_clock(id, cx);
        })
    });
    // Draws a frame, then tells whether cards are held and whether the card's clock runs.
    let held = |h: &mut PanesHarness| {
        h.cx.update(|window, cx| window.draw(cx).clear(cx));
        workspace.read_with(&h.cx, |workspace, _| {
            let cards = &workspace.cards;
            assert_eq!(cards.shown, Some(id), "the hovered card stays open, hidden");
            (cards.held, cards.clock.is_some())
        })
    };
    // The pointer leaves the row, then enters it again.
    let rehover = |h: &mut PanesHarness| {
        h.cx.update(|_, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.card_closed(id, cx);
                workspace.card_opened(id, cx);
            })
        });
    };
    let suppressed = |h: &mut PanesHarness| {
        workspace.read_with(&h.cx, |workspace, _| workspace.cards.suppressed)
    };
    assert_eq!(held(&mut h), (false, true), "nothing open: the card shows");

    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.start_rename(RenameTarget::Tab(id), window, cx)
        })
    });
    assert_eq!(held(&mut h), (true, false), "a name editor hides the card");
    h.cx.update(|window, cx| {
        workspace.update(cx, |workspace, cx| workspace.cancel_rename(window, cx))
    });
    assert_eq!(
        held(&mut h),
        (false, false),
        "the still hovered card stays hidden, with no clock"
    );
    assert_eq!(suppressed(&mut h), Some(id));
    rehover(&mut h);
    assert_eq!(
        held(&mut h),
        (false, true),
        "leave and enter: it shows again"
    );
    assert_eq!(suppressed(&mut h), None);

    let builder = WorkspaceView::tab_menu_builder(workspace.downgrade(), id);
    let open_menu = |h: &mut PanesHarness| -> Entity<PopupMenu> {
        let builder = builder.clone();
        h.cx.update(|window, cx| {
            PopupMenu::build(window, cx, move |menu, window, cx| {
                builder(menu, window, cx)
            })
        })
    };
    let menu = open_menu(&mut h);
    assert_eq!(held(&mut h), (true, false), "the tab menu hides the card");
    h.cx.update(|_, cx| menu.update(cx, |_, cx| cx.emit(DismissEvent)));
    assert_eq!(
        held(&mut h),
        (false, false),
        "dismissed, though still alive"
    );
    rehover(&mut h);
    let menu = open_menu(&mut h);
    assert!(held(&mut h).0);
    drop(menu);
    assert!(!held(&mut h).0, "a dropped menu (its row went) is closed");
    rehover(&mut h);

    h.cx.update(|window, cx| window.open_alert_dialog(cx, |alert, _, _| alert.title("Close?")));
    assert_eq!(held(&mut h), (true, false), "a dialog hides the card");
    h.cx.update(|window, cx| window.close_dialog(cx));
    assert_eq!(held(&mut h), (false, false));
    assert_eq!(suppressed(&mut h), Some(id));
    // A hold starting again keeps the card suppressed.
    h.cx.update(|window, cx| window.open_alert_dialog(cx, |alert, _, _| alert.title("Again?")));
    assert_eq!(held(&mut h), (true, false));
    h.cx.update(|window, cx| window.close_dialog(cx));
    assert_eq!(held(&mut h), (false, false));
    assert_eq!(suppressed(&mut h), Some(id));
}
