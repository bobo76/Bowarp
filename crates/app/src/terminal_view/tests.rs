use super::*;
use gpui_kit::Subscription;
use term_core::Palette;
use workspace_model::agents::{CLAUDE, COPILOT};

#[test]
fn only_a_hidden_finished_turn_is_ready_for_review() {
    use AgentStatus::*;
    assert!(finished_unseen(Working, Idle, false));
    assert!(!finished_unseen(Working, Idle, true));
    assert!(!finished_unseen(NeedsAttention, Idle, false));
    assert!(!finished_unseen(Working, NeedsAttention, false));
    assert!(!finished_unseen(Working, Exited, false));
}

#[gpui::test]
fn unread_completions_are_acknowledged_and_replaced_for_both_agents(
    cx: &mut gpui_kit::TestAppContext,
) {
    use agent_status::HookEvent;

    for kind in [SessionKind::Agent(CLAUDE), SessionKind::Agent(COPILOT)] {
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/C", "exit 0"]);
        #[cfg(not(windows))]
        let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
        let spec = LaunchSpec::new(
            kind,
            Some(program.into()),
            args.into_iter().map(String::from).collect(),
            None,
        );
        let session = Session::spawn(
            1,
            &spec,
            TermDimensions::new(80, 24),
            Palette::default(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let view = cx.new(|cx| {
            TerminalView::new(
                session,
                &spec,
                StatusReporting::Hooks,
                "sh".into(),
                Task::ready(()),
                cx,
            )
        });
        view.update(cx, |view, cx| {
            let mut at = 0;
            let mut report = |view: &mut TerminalView, working, cx: &mut Context<TerminalView>| {
                at += 1;
                if kind.has_activity_hooks() {
                    view.apply_hook_event(
                        AgentEvent::Hook(if working {
                            HookEvent::PromptSubmitted
                        } else {
                            HookEvent::Stopped
                        }),
                        at,
                        cx,
                    )
                } else if kind.has_progress_status() {
                    view.apply_progress(
                        if working {
                            ProgressState::Indeterminate
                        } else {
                            ProgressState::Hidden
                        },
                        at,
                        cx,
                    )
                } else {
                    unreachable!()
                }
            };
            report(view, true, cx);
            report(view, false, cx);
            assert_eq!(view.tab_status(), TabStatus::Idle);

            view.hide();
            report(view, true, cx);
            report(view, false, cx);
            assert_eq!(view.tab_status(), TabStatus::ReadyForReview);
            report(view, false, cx);
            view.on_session_event(SessionEvent::TitleChanged(Some("Agent".into())), cx);
            view.on_session_event(SessionEvent::Wakeup, cx);
            view.apply_hook_event(
                AgentEvent::ModeChanged(agent_status::AgentMode::Plan),
                0,
                cx,
            );
            assert_eq!(view.tab_status(), TabStatus::ReadyForReview);

            view.show(cx);
            assert_eq!(view.tab_status(), TabStatus::Idle);
            view.hide();
            report(view, false, cx);
            assert_eq!(view.tab_status(), TabStatus::Idle);

            report(view, true, cx);
            report(view, false, cx);
            report(view, true, cx);
            assert_eq!(view.tab_status(), TabStatus::Working);
            assert!(!view.finished_unseen);
            report(view, false, cx);
            assert_eq!(view.tab_status(), TabStatus::ReadyForReview);
            view.apply_hook_event(
                AgentEvent::Hook(HookEvent::InputRequested(WaitReason::Permission)),
                agent_status::now_micros(),
                cx,
            );
            assert_eq!(view.tab_status(), TabStatus::NeedsAttention);
            assert!(!view.finished_unseen);
        });
    }
}

#[test]
fn agent_notices_classify_status_transitions() {
    use AgentStatus::*;
    for before in [Idle, Working, NeedsAttention, Exited] {
        for after in [Idle, Working, NeedsAttention, Exited] {
            for reason in [None, Some(WaitReason::Permission), Some(WaitReason::Reply)] {
                let expected = match (before, after) {
                    (Working, Idle) => Some(AgentNotice::Finished),
                    (before, NeedsAttention) if before != NeedsAttention => {
                        Some(AgentNotice::NeedsAttention(reason))
                    }
                    _ => None,
                };
                assert_eq!(AgentNotice::for_change(before, after, reason), expected);
            }
        }
    }
}

#[gpui::test]
fn repeated_hooks_titles_and_output_do_not_repeat_agent_notices(cx: &mut gpui_kit::TestAppContext) {
    use std::cell::RefCell;
    use std::rc::Rc;

    use agent_status::HookEvent;

    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let spec = LaunchSpec::new(
        SessionKind::Agent(CLAUDE),
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        None,
    );
    let session = Session::spawn(
        1,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(|_| {}),
    )
    .unwrap();
    let view = cx.new(|cx| {
        TerminalView::new(
            session,
            &spec,
            StatusReporting::Hooks,
            "sh".into(),
            Task::ready(()),
            cx,
        )
    });
    let notices = Rc::new(RefCell::new(Vec::new()));
    let captured = notices.clone();
    let _events = cx.update(|cx| {
        cx.subscribe(&view, move |_, notice: &AgentNotice, _| {
            captured.borrow_mut().push(*notice);
        })
    });
    view.update(cx, |view, cx| {
        for event in [
            HookEvent::PromptSubmitted,
            HookEvent::InputRequested(WaitReason::Permission),
            HookEvent::ToolStarted,
            HookEvent::Stopped,
        ] {
            view.apply_hook_event(AgentEvent::Hook(event), 0, cx);
        }
        assert!(
            notices.borrow().is_empty(),
            "foreground tabs must not notify"
        );
        view.hide();
        for (event, at) in [
            (HookEvent::PromptSubmitted, 1),
            (HookEvent::InputRequested(WaitReason::Reply), 2),
            (HookEvent::InputRequested(WaitReason::Permission), 3),
            (HookEvent::ToolStarted, 4),
            (HookEvent::Stopped, 5),
            (HookEvent::Stopped, 6),
        ] {
            view.apply_hook_event(AgentEvent::Hook(event), at, cx);
            view.on_session_event(SessionEvent::TitleChanged(Some("Agent".into())), cx);
            view.on_session_event(SessionEvent::Wakeup, cx);
        }
        // A clean exit must not repeat the already reported finished turn.
        view.on_session_event(SessionEvent::Exited(Some(0)), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        *notices.borrow(),
        [
            AgentNotice::NeedsAttention(Some(WaitReason::Reply)),
            AgentNotice::Finished
        ]
    );
}

#[test]
fn default_bindings_name_terminal_actions() {
    let bindings = default_bindings();
    assert_eq!(bindings.len(), 9);
    assert!(
        bindings
            .iter()
            .all(|shortcut| shortcut.action.starts_with("terminal::"))
    );
}

#[gpui::test]
fn a_copilot_bell_keeps_the_pending_wait_reason(cx: &mut gpui_kit::TestAppContext) {
    use std::cell::RefCell;
    use std::rc::Rc;

    use agent_status::HookEvent;

    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let spec = LaunchSpec::new(
        SessionKind::Agent(COPILOT),
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        None,
    );
    let session = Session::spawn(
        3,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(|_| {}),
    )
    .unwrap();
    let view = cx.new(|cx| {
        TerminalView::new(
            session,
            &spec,
            StatusReporting::OutputActivity,
            "sh".into(),
            Task::ready(()),
            cx,
        )
    });
    let notices = Rc::new(RefCell::new(Vec::new()));
    let captured = notices.clone();
    let _events = cx.update(|cx| {
        cx.subscribe(&view, move |_, notice: &AgentNotice, _| {
            captured.borrow_mut().push(*notice);
        })
    });
    view.update(cx, |view, cx| {
        view.hide();
        view.on_session_event(SessionEvent::Bell, cx);
        view.show(cx);
        view.hide();
        view.apply_hook_event(
            AgentEvent::Hook(HookEvent::InputRequested(WaitReason::Permission)),
            1,
            cx,
        );
        view.on_session_event(SessionEvent::Bell, cx);
    });
    cx.run_until_parked();
    let permission = AgentNotice::NeedsAttention(Some(WaitReason::Permission));
    assert_eq!(
        *notices.borrow(),
        [AgentNotice::NeedsAttention(None), permission, permission]
    );
}

#[gpui::test]
fn copilot_hooks_add_and_end_waiting_but_only_progress_controls_working_and_idle(
    cx: &mut gpui_kit::TestAppContext,
) {
    use agent_status::HookEvent;

    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let spec = LaunchSpec::new(
        SessionKind::Agent(COPILOT),
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        None,
    );
    let session = Session::spawn(
        2,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(|_| {}),
    )
    .unwrap();
    let view = cx.new(|cx| {
        TerminalView::new(
            session,
            &spec,
            StatusReporting::OutputActivity,
            "sh".into(),
            Task::ready(()),
            cx,
        )
    });
    view.update(cx, |view, cx| {
        assert!(view.agent.is_none());
        view.apply_hook_event(AgentEvent::Hook(HookEvent::ToolStarted), 0, cx);
        assert!(view.agent.is_none(), "Copilot ignores activity hooks");
        view.apply_hook_event(
            AgentEvent::Hook(HookEvent::InputRequested(WaitReason::Permission)),
            0,
            cx,
        );
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::NeedsAttention
        );
        view.apply_hook_event(AgentEvent::Hook(HookEvent::Stopped), 1, cx);
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::NeedsAttention
        );
        view.on_session_event(
            SessionEvent::ProgressChanged {
                state: ProgressState::Indeterminate,
                at: 2,
            },
            cx,
        );
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::NeedsAttention,
            "repeated progress keeps Waiting"
        );
        view.apply_hook_event(AgentEvent::Hook(HookEvent::ToolFinished), 3, cx);
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::Working,
            "a tool hook ends Waiting"
        );
        view.apply_hook_event(
            AgentEvent::Hook(HookEvent::InputRequested(WaitReason::Reply)),
            4,
            cx,
        );
        view.on_session_event(
            SessionEvent::ProgressChanged {
                state: ProgressState::Hidden,
                at: 3,
            },
            cx,
        );
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::NeedsAttention,
            "queued older progress cannot clear a newer question"
        );
        view.apply_progress(ProgressState::Hidden, 5, cx);
        assert_eq!(view.agent.as_ref().unwrap().status(), AgentStatus::Idle);
        view.apply_hook_event(
            AgentEvent::Hook(HookEvent::InputRequested(WaitReason::Permission)),
            0,
            cx,
        );
        assert_eq!(
            view.agent.as_ref().unwrap().status(),
            AgentStatus::Idle,
            "a delayed permission hook cannot override completed progress"
        );
        view.on_session_event(SessionEvent::Exited(Some(0)), cx);
        view.apply_hook_event(
            AgentEvent::Hook(HookEvent::InputRequested(WaitReason::Permission)),
            agent_status::now_micros(),
            cx,
        );
        assert_eq!(view.agent.as_ref().unwrap().status(), AgentStatus::Exited);
    });
}

fn copilot_view_with_mode_args(
    cx: &mut gpui_kit::TestAppContext,
    mode_args: &[&str],
) -> Entity<TerminalView> {
    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let spec = LaunchSpec::new(
        SessionKind::Agent(COPILOT),
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        None,
    );
    let session = Session::spawn(
        3,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(|_| {}),
    )
    .unwrap();
    let mut launch = spec.clone();
    launch.args = mode_args.iter().map(|arg| (*arg).into()).collect();
    cx.new(|cx| {
        TerminalView::new(
            session,
            &launch,
            StatusReporting::OutputActivity,
            "sh".into(),
            Task::ready(()),
            cx,
        )
    })
}

#[gpui::test]
fn copilot_autopilot_permissions_do_not_adopt_status_or_emit_attention(
    cx: &mut gpui_kit::TestAppContext,
) {
    use agent_status::HookEvent;
    use std::cell::Cell;
    use std::rc::Rc;

    let view = copilot_view_with_mode_args(cx, &["--mode=plan", "--mode", "autopilot"]);
    let notices = Rc::new(Cell::new(0));
    let seen = notices.clone();
    let _events =
        cx.update(|cx| cx.subscribe(&view, move |_, _: &AgentNotice, _| seen.set(seen.get() + 1)));
    view.update(cx, |view, cx| {
        view.hide();
        let permission = AgentEvent::Hook(HookEvent::InputRequested(WaitReason::Permission));
        view.apply_hook_event(permission, 1, cx);
        assert!(
            view.agent.is_none(),
            "an automatic approval must not adopt Idle status"
        );
        view.apply_progress(ProgressState::Indeterminate, 2, cx);
        view.apply_hook_event(permission, 3, cx);
        assert_eq!(view.tab_status(), TabStatus::Working);
        view.apply_hook_event(
            AgentEvent::Hook(HookEvent::InputRequested(WaitReason::Reply)),
            4,
            cx,
        );
        assert_eq!(view.tab_status(), TabStatus::NeedsAttention);
        view.apply_hook_event(AgentEvent::Hook(HookEvent::ToolStarted), 5, cx);
        view.apply_hook_event(AgentEvent::ModeChanged(AgentMode::Interactive), 6, cx);
        view.apply_hook_event(permission, 7, cx);
        assert_eq!(view.tab_status(), TabStatus::NeedsAttention);
    });
    cx.run_until_parked();
    assert_eq!(
        notices.get(),
        2,
        "only the question and interactive permission notify"
    );
}

#[gpui::test]
fn copilot_initial_mode_precedence_controls_permission_attention(
    cx: &mut gpui_kit::TestAppContext,
) {
    use agent_status::HookEvent;
    use std::cell::Cell;
    use std::rc::Rc;

    for (args, expected) in [
        (vec!["--plan", "--mode", "autopilot"], Some(AgentMode::Plan)),
        (vec!["--mode=autopilot", "--plan"], Some(AgentMode::Plan)),
        (
            vec!["--mode=autopilot", "--mode=interactive"],
            Some(AgentMode::Interactive),
        ),
        (
            vec!["--mode=plan", "--mode=autopilot"],
            Some(AgentMode::Autopilot),
        ),
        (
            vec!["--autopilot", "--autopilot"],
            Some(AgentMode::Autopilot),
        ),
        (vec!["--plan", "--autopilot"], None),
        (vec!["--plan", "--mode=interactive"], None),
    ] {
        let view = copilot_view_with_mode_args(cx, &args);
        let notices = Rc::new(Cell::new(0));
        let seen = notices.clone();
        let _events = cx.update(|cx| {
            cx.subscribe(&view, move |_, _: &AgentNotice, _| seen.set(seen.get() + 1))
        });
        view.update(cx, |view, cx| {
            assert_eq!(view.agent_mode(), expected, "{args:?}");
            view.hide();
            view.apply_progress(ProgressState::Indeterminate, 1, cx);
            view.apply_hook_event(
                AgentEvent::Hook(HookEvent::InputRequested(WaitReason::Permission)),
                2,
                cx,
            );
            assert_eq!(
                view.tab_status(),
                if expected == Some(AgentMode::Autopilot) {
                    TabStatus::Working
                } else {
                    TabStatus::NeedsAttention
                },
                "{args:?}"
            );
        });
        cx.run_until_parked();
        assert_eq!(
            notices.get(),
            usize::from(expected != Some(AgentMode::Autopilot)),
            "{args:?}"
        );
    }
}

#[gpui::test]
fn clipboard_writes_wait_for_an_answer(cx: &mut gpui_kit::TestAppContext) {
    use term_core::alacritty_terminal::term::ClipboardType;

    use workspace_model::{ClipboardAnswer, ClipboardGuard, ClipboardPolicy};

    cx.update(settings_file::init_in_memory);
    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let spec = LaunchSpec::new(
        SessionKind::Shell,
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        None,
    );
    let session = Session::spawn(
        1,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(|_| {}),
    )
    .unwrap();
    let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
    let clipboard_text =
        |cx: &mut gpui_kit::TestAppContext| cx.read_from_clipboard().and_then(|item| item.text());
    let store = |text: &str| SessionEvent::ClipboardStore(ClipboardType::Clipboard, text.into());

    view.update(cx, |view, cx| {
        view.dispatch_test_event(store("first"), cx);
        view.dispatch_test_event(store("second"), cx);
        assert!(view.clipboard_pending());
        assert_eq!(view.tab_status(), TabStatus::NeedsAttention);
    });
    assert_eq!(clipboard_text(cx), None, "held until allowed");
    view.update(cx, |view, cx| {
        view.answer_clipboard(ClipboardAnswer::Allow, cx)
    });
    assert_eq!(clipboard_text(cx).as_deref(), Some("second"));

    view.update(cx, |view, cx| {
        assert!(!view.clipboard_pending());
        assert_ne!(view.tab_status(), TabStatus::NeedsAttention);
        view.dispatch_test_event(store("denied"), cx);
        view.answer_clipboard(ClipboardAnswer::Deny, cx);
        assert!(!view.clipboard_pending());
    });
    assert_eq!(clipboard_text(cx).as_deref(), Some("second"));

    view.update(cx, |view, cx| {
        view.dispatch_test_event(store("always"), cx);
        view.answer_clipboard(ClipboardAnswer::Always, cx);
        view.dispatch_test_event(store("later"), cx);
        assert!(!view.clipboard_pending());
        assert!(view.clipboard_always());
    });
    assert_eq!(clipboard_text(cx).as_deref(), Some("later"));

    let carried = view.read_with(cx, |view, _| view.clipboard_carry_over());
    view.update(cx, |view, cx| {
        view.set_clipboard_guard(carried);
        view.dispatch_test_event(store("carried"), cx);
        assert!(!view.clipboard_pending());
        assert!(view.clipboard_always());
    });
    assert_eq!(clipboard_text(cx).as_deref(), Some("carried"));

    let set_policy = |policy, cx: &mut gpui_kit::TestAppContext| {
        cx.update(|cx| settings_file::update(cx, |settings| settings.clipboard_writes = policy));
    };
    set_policy(ClipboardPolicy::Deny, cx);
    view.update(cx, |view, cx| {
        view.dispatch_test_event(store("denied by the setting"), cx);
        assert!(!view.clipboard_pending());
    });
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("carried"),
        "Deny overrides Always for this tab"
    );
    set_policy(ClipboardPolicy::Allow, cx);
    view.update(cx, |view, cx| {
        view.set_clipboard_guard(ClipboardGuard::default());
        view.dispatch_test_event(store("allowed by the setting"), cx);
        assert!(!view.clipboard_pending());
    });
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("allowed by the setting")
    );
    set_policy(ClipboardPolicy::Ask, cx);

    #[cfg(target_os = "linux")]
    view.update(cx, |view, cx| {
        view.set_clipboard_guard(ClipboardGuard::default());
        let selection = ClipboardType::Selection;
        view.dispatch_test_event(SessionEvent::ClipboardStore(selection, "sel".into()), cx);
        assert!(
            view.clipboard_pending(),
            "a primary selection write is held too"
        );
        view.answer_clipboard(ClipboardAnswer::Deny, cx);
        assert!(!view.clipboard_pending());
    });

    view.update(cx, |view, cx| {
        view.set_clipboard_guard(ClipboardGuard::default());
        view.dispatch_test_event(store("at exit"), cx);
        view.dispatch_test_event(SessionEvent::Exited(Some(0)), cx);
        assert!(
            !view.clipboard_pending(),
            "an exited program's write is dropped"
        );
        view.dispatch_test_event(store("after exit"), cx);
        assert!(!view.clipboard_pending());
    });
    assert_eq!(
        clipboard_text(cx).as_deref(),
        Some("allowed by the setting")
    );
}

#[gpui::test]
fn a_quiet_claude_tab_without_hooks_shows_status_unavailable(cx: &mut gpui_kit::TestAppContext) {
    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    for (kind, quiet) in [
        (SessionKind::Agent(CLAUDE), TabStatus::Unavailable),
        (SessionKind::Agent(COPILOT), TabStatus::Quiet),
        (SessionKind::Shell, TabStatus::Quiet),
    ] {
        let spec = LaunchSpec::new(
            kind,
            Some(program.into()),
            args.iter().copied().map(String::from).collect(),
            None,
        );
        let session = Session::spawn(
            4,
            &spec,
            TermDimensions::new(80, 24),
            Palette::default(),
            Arc::new(|_| {}),
        )
        .unwrap();
        let view = cx.new(|cx| {
            TerminalView::new(
                session,
                &spec,
                StatusReporting::OutputActivity,
                "sh".into(),
                Task::ready(()),
                cx,
            )
        });
        view.update(cx, |view, cx| {
            assert_eq!(view.tab_status(), quiet, "{kind:?}");
            view.hide();
            view.on_session_event(SessionEvent::Bell, cx);
            assert_eq!(
                view.tab_status(),
                TabStatus::NeedsAttention,
                "{kind:?}: without hooks a bell asks for the user"
            );
        });
    }
}

/// A shell tab whose environment carries the agents' hooks, hidden, with its notices
/// collected.
fn in_place_shell(
    cx: &mut gpui_kit::TestAppContext,
) -> (
    Entity<TerminalView>,
    std::rc::Rc<std::cell::RefCell<Vec<AgentNotice>>>,
    Subscription,
) {
    in_place_shell_with_hooks(cx, &[CLAUDE, COPILOT])
}

/// [`in_place_shell`] with the hooks of only `hooked` in the environment.
fn in_place_shell_with_hooks(
    cx: &mut gpui_kit::TestAppContext,
    hooked: &[AgentId],
) -> (
    Entity<TerminalView>,
    std::rc::Rc<std::cell::RefCell<Vec<AgentNotice>>>,
    Subscription,
) {
    #[cfg(windows)]
    let (program, args) = ("cmd", vec!["/C", "exit 0"]);
    #[cfg(not(windows))]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 0"]);
    let mut spec = LaunchSpec::new(
        SessionKind::Shell,
        Some(program.into()),
        args.into_iter().map(String::from).collect(),
        None,
    );
    for agent in hooked {
        spec.env.insert(
            agent.hook_artifact_plan().shell_env.into(),
            "artifact".into(),
        );
    }
    let session = Session::spawn(
        5,
        &spec,
        TermDimensions::new(80, 24),
        Palette::default(),
        Arc::new(|_| {}),
    )
    .unwrap();
    let view = cx.new(|cx| {
        TerminalView::new(
            session,
            &spec,
            StatusReporting::OutputActivity,
            "sh".into(),
            Task::ready(()),
            cx,
        )
    });
    let notices = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = notices.clone();
    let events = cx.update(|cx| {
        cx.subscribe(&view, move |_, notice: &AgentNotice, _| {
            captured.borrow_mut().push(*notice);
        })
    });
    view.update(cx, |view, cx| {
        view.on_session_event(SessionEvent::TitleChanged(Some("bash".into())), cx);
        view.hide();
    });
    (view, notices, events)
}

fn report(name: &str) -> AgentReport {
    AgentReport {
        event_name: name.into(),
        detail: None,
    }
}

#[gpui::test]
fn an_in_place_claude_run_shows_agent_status_until_its_d_mark(cx: &mut gpui_kit::TestAppContext) {
    let (view, notices, _events) = in_place_shell(cx);
    view.update(cx, |view, cx| {
        // Hooks before the mark are another program's: ignored.
        view.apply_report(&report("UserPromptSubmit"), 1, cx);
        assert_eq!(view.tab_status(), TabStatus::Quiet);
        view.on_session_event(SessionEvent::CommandStarted(Some("claude".into())), cx);
        view.on_session_event(SessionEvent::AgentStarted(CLAUDE), cx);
        assert_eq!(view.status_kind(), SessionKind::Agent(CLAUDE));
        assert_eq!(view.kind(), SessionKind::Shell, "saved as a shell");
        assert_eq!(view.tab_status(), TabStatus::Idle);
        assert_eq!(
            view.busy_reason(),
            None,
            "an idle agent closes without asking"
        );
        view.on_session_event(SessionEvent::TitleChanged(Some("✳ Claude Code".into())), cx);
        view.apply_report(&report("UserPromptSubmit"), 2, cx);
        assert_eq!(view.tab_status(), TabStatus::Working);
        assert_eq!(view.busy_reason(), Some(BusyReason::AgentWorking));
        view.apply_report(&report("Stop"), 3, cx);
        assert_eq!(view.tab_status(), TabStatus::ReadyForReview);
        // Claude's hooks report attention: its bell is ignored.
        view.on_session_event(SessionEvent::Bell, cx);
        assert_eq!(view.tab_status(), TabStatus::ReadyForReview);
        // Ctrl+C: the shell's D mark with status 130.
        view.on_session_event(SessionEvent::CommandFinished(Some(130)), cx);
        assert_eq!(view.status_kind(), SessionKind::Shell);
        assert_eq!(view.name(), "bash", "the title from before the run");
        assert_eq!(
            view.tab_status(),
            TabStatus::ReadyForReview,
            "kept until seen"
        );
        // Hooks after the run are ignored again.
        view.apply_report(&report("UserPromptSubmit"), 4, cx);
        assert_eq!(view.tab_status(), TabStatus::ReadyForReview);
        view.show(cx);
        assert_eq!(view.tab_status(), TabStatus::Quiet);
        assert_eq!(view.busy_reason(), None);
    });
    cx.run_until_parked();
    assert_eq!(
        *notices.borrow(),
        [AgentNotice::Finished, AgentNotice::Exited(Some(130))]
    );
}

#[gpui::test]
fn the_end_of_an_in_place_run_clears_attention(cx: &mut gpui_kit::TestAppContext) {
    let (view, notices, _events) = in_place_shell(cx);
    view.update(cx, |view, cx| {
        view.on_session_event(SessionEvent::CommandStarted(None), cx);
        view.on_session_event(SessionEvent::AgentStarted(CLAUDE), cx);
        view.apply_report(&report("PermissionRequest"), 1, cx);
        assert_eq!(view.tab_status(), TabStatus::NeedsAttention);
        assert_eq!(view.busy_reason(), Some(BusyReason::AgentWaiting));
        view.on_session_event(SessionEvent::CommandFinished(Some(0)), cx);
        assert_eq!(view.tab_status(), TabStatus::Quiet);
        // A plain command afterwards is a shell command again.
        view.on_session_event(SessionEvent::CommandStarted(Some("make".into())), cx);
        assert_eq!(
            view.busy_reason(),
            Some(BusyReason::Command(Some("make".into())))
        );
    });
    cx.run_until_parked();
    assert_eq!(
        *notices.borrow(),
        [
            AgentNotice::NeedsAttention(Some(WaitReason::Permission)),
            AgentNotice::Exited(Some(0))
        ]
    );
}

#[gpui::test]
fn a_shell_exiting_mid_run_leaves_an_exited_shell_tab(cx: &mut gpui_kit::TestAppContext) {
    let (view, notices, _events) = in_place_shell(cx);
    view.update(cx, |view, cx| {
        view.on_session_event(SessionEvent::CommandStarted(Some("copilot".into())), cx);
        view.on_session_event(SessionEvent::AgentStarted(COPILOT), cx);
        assert_eq!(view.status_kind(), SessionKind::Agent(COPILOT));
        // Copilot's status starts with its first progress report; its bell still asks.
        view.on_session_event(
            SessionEvent::ProgressChanged {
                state: ProgressState::Indeterminate,
                at: 1,
            },
            cx,
        );
        assert_eq!(view.tab_status(), TabStatus::Working);
        view.on_session_event(
            SessionEvent::ProgressChanged {
                state: ProgressState::Hidden,
                at: 2,
            },
            cx,
        );
        assert_eq!(view.tab_status(), TabStatus::ReadyForReview);
        view.on_session_event(SessionEvent::Exited(Some(1)), cx);
        assert_eq!(view.status_kind(), SessionKind::Shell);
        assert_eq!(view.tab_status(), TabStatus::Exited, "Exited wins");
    });
    cx.run_until_parked();
    assert_eq!(
        *notices.borrow(),
        [AgentNotice::Finished, AgentNotice::Exited(Some(1))]
    );
}

#[gpui::test]
fn a_second_agent_mark_in_one_run_starts_over_keeping_the_first_title(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (view, _notices, _events) = in_place_shell(cx);
    view.update(cx, |view, cx| {
        view.on_session_event(
            SessionEvent::CommandStarted(Some("claude; copilot".into())),
            cx,
        );
        view.on_session_event(SessionEvent::AgentStarted(CLAUDE), cx);
        view.on_session_event(SessionEvent::TitleChanged(Some("✳ Claude Code".into())), cx);
        view.apply_report(&report("UserPromptSubmit"), 1, cx);
        view.apply_report(&report("Stop"), 2, cx);
        assert_eq!(view.tab_status(), TabStatus::ReadyForReview);
        assert!(view.finished_unseen);

        view.on_session_event(SessionEvent::AgentStarted(COPILOT), cx);
        assert_eq!(view.status_kind(), SessionKind::Agent(COPILOT));
        assert!(!view.finished_unseen, "the first agent's result is dropped");
        assert!(
            view.agent.is_none(),
            "Copilot has no tracker before progress"
        );
        assert_ne!(view.tab_status(), TabStatus::ReadyForReview);

        view.on_session_event(SessionEvent::CommandFinished(Some(0)), cx);
        assert_eq!(view.name(), "bash", "the title from before the first mark");
    });
}

#[gpui::test]
fn an_agent_mark_after_the_shell_exited_is_ignored(cx: &mut gpui_kit::TestAppContext) {
    let (view, _notices, _events) = in_place_shell(cx);
    view.update(cx, |view, cx| {
        view.on_session_event(SessionEvent::Exited(Some(0)), cx);
        view.on_session_event(SessionEvent::AgentStarted(CLAUDE), cx);
        assert_eq!(view.status_kind(), SessionKind::Shell);
        assert_eq!(view.tab_status(), TabStatus::Exited);
    });
}

#[gpui::test]
fn an_in_place_claude_without_its_settings_has_no_status(cx: &mut gpui_kit::TestAppContext) {
    let (view, _notices, _events) = in_place_shell_with_hooks(cx, &[COPILOT]);
    view.update(cx, |view, cx| {
        view.on_session_event(SessionEvent::CommandStarted(Some("claude".into())), cx);
        view.on_session_event(SessionEvent::AgentStarted(CLAUDE), cx);
        assert_eq!(view.status_kind(), SessionKind::Agent(CLAUDE));
        assert!(view.agent.is_none());
        assert_eq!(view.tab_status(), TabStatus::Unavailable);
        // Its hooks, if any reached the tab, are not tracked.
        view.apply_report(&report("UserPromptSubmit"), 1, cx);
        assert_eq!(view.tab_status(), TabStatus::Unavailable);
    });
}
