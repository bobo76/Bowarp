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
    assert_eq!(bindings.len(), 11);
    let keyless: Vec<_> = bindings
        .iter()
        .filter(|shortcut| shortcut.keys.is_empty())
        .map(|shortcut| shortcut.action)
        .collect();
    assert_eq!(
        keyless,
        ["terminal::SelectAll", "terminal::ClearScrollback"]
    );
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

fn line(text: &str) -> AgentReport {
    let mut fields = text.splitn(2, ' ');
    let event_name = fields.next().unwrap().to_owned();
    let mut report = AgentReport {
        event_name,
        ..AgentReport::default()
    };
    for field in fields.next().unwrap_or_default().split(' ') {
        if let Some(id) = field.strip_prefix("agent=") {
            report.subagent = Some(id.into());
        } else if let Some(kind) = field.strip_prefix("kind=") {
            report.subagent_kind = Some(kind.into());
        } else if let Some(description) = field.strip_prefix("desc=") {
            report.description = Some(description.into());
        } else if !field.is_empty() {
            report.detail = Some(field.into());
        }
    }
    report
}

#[gpui::test]
fn subagents_show_from_start_to_stop_and_end_with_the_agent(cx: &mut gpui_kit::TestAppContext) {
    let view = copilot_view_with_mode_args(cx, &[]);
    view.update(cx, |view, cx| {
        view.apply_report(&line("PreToolUse desc=List"), 1, cx);
        view.apply_report(&line("SubagentStart agent=a1 kind=explore"), 2, cx);
        view.apply_report(&line("SubagentStart agent=a2 kind=explore"), 3, cx);
        let labels: Vec<_> = view
            .subagents()
            .iter()
            .map(|s| s.label().to_owned())
            .collect();
        assert_eq!(labels, ["List", "explore"]);
        assert_eq!(
            view.tab_status(),
            TabStatus::Quiet,
            "the main status stays its own"
        );

        view.apply_report(&line("PermissionRequest agent=a2"), 4, cx);
        assert!(view.subagents()[1].waiting);
        view.apply_report(&line("PostToolUse agent=a2"), 5, cx);
        assert!(!view.subagents()[1].waiting);
        view.apply_report(&line("SubagentStop agent=a1"), 6, cx);
        assert_eq!(view.subagents().len(), 1);

        view.on_session_event(SessionEvent::Exited(Some(1)), cx);
        assert!(view.subagents().is_empty());
        view.apply_report(&line("SubagentStart agent=a3"), 7, cx);
        assert!(view.subagents().is_empty(), "nothing runs after the exit");
    });
}

#[gpui::test]
fn autopilot_subagents_do_not_wait_for_permissions(cx: &mut gpui_kit::TestAppContext) {
    let view = copilot_view_with_mode_args(cx, &["--autopilot"]);
    view.update(cx, |view, cx| {
        view.apply_report(&line("SubagentStart agent=a1 kind=explore"), 1, cx);
        view.apply_report(&line("PermissionRequest agent=a1"), 2, cx);
        assert!(!view.subagents()[0].waiting);
        view.apply_report(&line("Notification elicitation_dialog agent=a1"), 3, cx);
        assert!(view.subagents()[0].waiting, "questions still wait");
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
        ..AgentReport::default()
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
fn an_agent_end_mark_ends_the_run_before_the_command_line_does(cx: &mut gpui_kit::TestAppContext) {
    let (view, notices, _events) = in_place_shell(cx);
    view.update(cx, |view, cx| {
        view.on_session_event(
            SessionEvent::CommandStarted(Some("claude; make".into())),
            cx,
        );
        view.on_session_event(SessionEvent::AgentStarted(CLAUDE), cx);
        view.on_session_event(SessionEvent::TitleChanged(Some("✳ Claude Code".into())), cx);
        view.apply_report(&report("UserPromptSubmit"), 1, cx);
        assert_eq!(view.tab_status(), TabStatus::Working);
        view.on_session_event(SessionEvent::AgentFinished(Some(3)), cx);
        assert_eq!(view.status_kind(), SessionKind::Shell);
        assert_eq!(view.name(), "bash", "the title from before the run");
        // `make` runs on as a shell command.
        assert_eq!(
            view.busy_reason(),
            Some(BusyReason::Command(Some("claude; make".into())))
        );
        // The command line's D mark ends nothing more.
        view.on_session_event(SessionEvent::CommandFinished(Some(0)), cx);
        assert_eq!(view.tab_status(), TabStatus::Quiet);
        assert_eq!(view.busy_reason(), None);
        // An end mark without a run is ignored.
        view.on_session_event(SessionEvent::AgentFinished(Some(1)), cx);
    });
    cx.run_until_parked();
    assert_eq!(*notices.borrow(), [AgentNotice::Exited(Some(3))]);
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
        assert_eq!(view.tab_status(), TabStatus::Exited(Some(1)), "Exited wins");
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
        assert_eq!(view.tab_status(), TabStatus::Exited(Some(0)));
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

#[test]
fn the_disconnect_banner_says_what_happened_and_offers_the_next_step() {
    use remote::DisconnectPhase;

    let mut disconnect = Disconnect::lost("ssh: broken pipe".into());
    assert_eq!(
        disconnect.message("box"),
        "Connection to box lost: ssh: broken pipe"
    );
    assert_eq!(
        disconnect.action(),
        Some(("Reconnect", RemoteAction::Reconnect))
    );
    disconnect.phase = DisconnectPhase::Reconnecting;
    assert_eq!(disconnect.message("box"), "Reconnecting to box…");
    assert_eq!(disconnect.action(), None);
    disconnect.phase = DisconnectPhase::Gone;
    assert!(
        disconnect
            .message("box")
            .starts_with("The session is gone from box")
    );
    assert_eq!(
        disconnect.action(),
        Some(("Start new", RemoteAction::StartNew))
    );
    disconnect.phase = DisconnectPhase::Starting;
    assert_eq!(disconnect.message("box"), "Starting a new session on box…");
    assert_eq!(disconnect.action(), None);
}

/// A remote session's link that sends nothing anywhere.
struct NoLink;

impl term_core::RemoteLink for NoLink {
    fn write(&self, _: Cow<'static, [u8]>) {}
    fn resize(&self, _: TermDimensions) {}
    fn close(&self) {}
    fn remote_id(&self) -> u64 {
        77
    }
}

#[gpui::test]
fn a_disconnected_remote_tab_reconnects_or_starts_new(cx: &mut gpui_kit::TestAppContext) {
    let mut spec = LaunchSpec::new(SessionKind::Shell, Some("sh".into()), Vec::new(), None);
    spec.host = Some("box".into());
    let (session, _feed) = Session::remote(
        1,
        SessionKind::Shell,
        TermDimensions::new(40, 5),
        Palette::default(),
        Arc::new(|_| {}),
        Box::new(NoLink),
    );
    let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
    view.update(cx, |view, cx| {
        assert!(!view.can_reconnect());
        view.on_session_event(SessionEvent::Disconnected("cut".into()), cx);
        assert_eq!(view.tab_status(), TabStatus::Disconnected);
        assert!(!view.at_prompt());
        assert!(view.can_reconnect());
        let (_, reattach) = view.begin_reconnect(2, &spec, cx).unwrap();
        assert_eq!(reattach.server_id, 77);
        assert_eq!(reattach.size, TermDimensions::new(40, 5));
        assert_eq!(reattach.view.id, 2);
        assert!(!view.can_reconnect(), "one reconnect at a time");
        assert!(view.begin_reconnect(3, &spec, cx).is_none());
        // Unreachable: back to Reconnect.
        view.end_reconnect(false, cx);
        assert!(view.can_reconnect());
        assert!(
            !view.begin_start_new(cx),
            "Start new only once the session is gone"
        );
        view.begin_reconnect(3, &spec, cx).unwrap();
        view.end_reconnect(true, cx);
        assert!(!view.can_reconnect());
        assert!(view.begin_start_new(cx));
        assert!(!view.begin_start_new(cx));
        assert_eq!(view.tab_status(), TabStatus::Disconnected);
    });
}

#[gpui::test]
fn a_restart_under_way_blocks_enter_until_it_ends(cx: &mut gpui_kit::TestAppContext) {
    use std::cell::Cell;
    use std::rc::Rc;

    let (view, _notices, _events) = in_place_shell(cx);
    let restarts = Rc::new(Cell::new(0));
    let counted = restarts.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&view, move |_, _: &RestartTab, _| {
            counted.set(counted.get() + 1)
        })
    });
    let press = |key: &str, is_held: bool| gpui_kit::KeyDownEvent {
        keystroke: gpui_kit::Keystroke::parse(key).unwrap(),
        is_held,
        prefer_character_input: false,
    };
    view.update(cx, |view, cx| {
        assert!(!view.begin_restart(cx), "a running tab does not restart");
        view.on_session_event(SessionEvent::Exited(Some(1)), cx);
        assert!(view.can_restart());
        assert!(!view.restart_on_enter(&press("ctrl-enter", false), cx));
        assert!(!view.restart_on_enter(&press("shift-enter", false), cx));
        assert!(
            !view.restart_on_enter(&press("enter", true), cx),
            "a held key"
        );
        assert!(view.begin_restart(cx));
        assert!(!view.can_restart());
        assert!(!view.restart_on_enter(&press("enter", false), cx));
        assert!(!view.begin_restart(cx), "only one at a time");
        view.end_restart(cx);
        assert!(view.can_restart());
        assert!(view.restart_on_enter(&press("enter", false), cx));
    });
    assert_eq!(restarts.get(), 1);
}

/// A remote session's link that records what is typed into it.
struct RecordingLink(Arc<std::sync::Mutex<Vec<u8>>>);

impl term_core::RemoteLink for RecordingLink {
    fn write(&self, bytes: Cow<'static, [u8]>) {
        self.0.lock().unwrap().extend_from_slice(&bytes);
    }
    fn resize(&self, _: TermDimensions) {}
    fn close(&self) {}
    fn remote_id(&self) -> u64 {
        78
    }
}

#[gpui::test]
fn a_paste_whose_lines_would_run_asks_first(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::{Root, WindowExt};

    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init_in_memory(cx);
        crate::terminal_element::init(cx);
    });
    let spec = LaunchSpec::new(SessionKind::Shell, Some("sh".into()), Vec::new(), None);
    let typed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (session, feed) = Session::remote(
        1,
        SessionKind::Shell,
        TermDimensions::new(40, 5),
        Palette::default(),
        Arc::new(|_| {}),
        Box::new(RecordingLink(typed.clone())),
    );
    let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
    let root = view.clone();
    let window = cx.add_window(move |window, cx| Root::new(root, window, cx));
    let mut cx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let paste = |cx: &mut gpui_kit::VisualTestContext, text: &str| {
        let text = Some(text.to_owned());
        cx.update(|window, cx| view.update(cx, |view, cx| view.paste(text, window, cx)));
        cx.run_until_parked();
    };
    let dialog =
        |cx: &mut gpui_kit::VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));
    let take = |typed: &Arc<std::sync::Mutex<Vec<u8>>>| std::mem::take(&mut *typed.lock().unwrap());

    let focused = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| view.read(cx).focus_handle.is_focused(window))
    };
    let handle = view.read_with(&cx, |view, _| view.focus_handle.clone());
    cx.update(|window, cx| handle.focus(window, cx));

    // One line goes straight through.
    paste(&mut cx, "echo hi");
    assert!(!dialog(&mut cx));
    assert_eq!(take(&typed), b"echo hi");

    // Lines ask; a second paste while asking does not ask again; Esc sends nothing.
    paste(&mut cx, "a\nb\n");
    assert!(dialog(&mut cx));
    paste(&mut cx, "c\nd\n");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!dialog(&mut cx), "one question only");
    assert_eq!(take(&typed), b"");
    assert!(focused(&mut cx), "the keyboard goes back to the terminal");

    // Enter pastes the text read when asked, once, in the mode current then.
    paste(&mut cx, "a\nb\n");
    feed.output(b"\x1b[?2004h");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(!dialog(&mut cx));
    assert_eq!(take(&typed), b"\x1b[200~a\nb\n\x1b[201~");
    assert!(focused(&mut cx));

    // With bracketed paste on, lines don't run: no question.
    paste(&mut cx, "a\nb\n");
    assert!(!dialog(&mut cx));
    assert_eq!(take(&typed), b"\x1b[200~a\nb\n\x1b[201~");

    // Off again, the question is asked again (no "don't ask again").
    feed.output(b"\x1b[?2004l");
    paste(&mut cx, "x\n");
    assert!(dialog(&mut cx));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(take(&typed), b"x\r");
}

#[cfg(target_os = "linux")]
#[gpui::test]
fn a_middle_click_paste_whose_lines_would_run_asks_first(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::{Root, WindowExt};

    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init_in_memory(cx);
        crate::terminal_element::init(cx);
    });
    let spec = LaunchSpec::new(SessionKind::Shell, Some("sh".into()), Vec::new(), None);
    let typed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (session, _feed) = Session::remote(
        1,
        SessionKind::Shell,
        TermDimensions::new(40, 5),
        Palette::default(),
        Arc::new(|_| {}),
        Box::new(RecordingLink(typed.clone())),
    );
    let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
    let root = view.clone();
    let window = cx.add_window(move |window, cx| Root::new(root, window, cx));
    let mut cx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let dialog =
        |cx: &mut gpui_kit::VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));
    let middle_click = |cx: &mut gpui_kit::VisualTestContext| {
        cx.simulate_mouse_down(
            gpui_kit::point(gpui_kit::px(20.0), gpui_kit::px(20.0)),
            gpui_kit::MouseButton::Middle,
            gpui_kit::Modifiers::default(),
        );
        cx.simulate_mouse_up(
            gpui_kit::point(gpui_kit::px(20.0), gpui_kit::px(20.0)),
            gpui_kit::MouseButton::Middle,
            gpui_kit::Modifiers::default(),
        );
        cx.run_until_parked();
    };

    let handle = view.read_with(&cx, |view, _| view.focus_handle.clone());
    cx.update(|window, cx| handle.focus(window, cx));
    cx.update(|_, cx| cx.write_to_primary(gpui_kit::ClipboardItem::new_string("a\nb\n".into())));
    middle_click(&mut cx);
    assert!(dialog(&mut cx), "the selection's lines ask first");
    assert!(
        typed.lock().unwrap().is_empty(),
        "nothing is sent before the answer"
    );

    // Esc reaches the dialog (a click must not leave the focus on the terminal), sends nothing.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!dialog(&mut cx));
    assert!(typed.lock().unwrap().is_empty());

    // Paste sends the selection once.
    middle_click(&mut cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(*typed.lock().unwrap(), b"a\rb\r".to_vec());
}

#[gpui::test]
fn right_click_opens_the_menu_unless_the_program_tracks_the_mouse(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::component::{Root, WindowExt};
    use gpui_kit::{Modifiers, MouseButton, point, px};

    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init_in_memory(cx);
        crate::terminal_element::init(cx);
    });
    let spec = LaunchSpec::new(SessionKind::Shell, Some("sh".into()), Vec::new(), None);
    let typed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (session, feed) = Session::remote(
        1,
        SessionKind::Shell,
        TermDimensions::new(40, 5),
        Palette::default(),
        Arc::new(|_| {}),
        Box::new(RecordingLink(typed.clone())),
    );
    let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
    let root = view.clone();
    let window = cx.add_window(move |window, cx| Root::new(root, window, cx));
    let mut cx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let right_click = |cx: &mut gpui_kit::VisualTestContext, modifiers: Modifiers| {
        let at = point(px(20.0), px(20.0));
        cx.simulate_mouse_down(at, MouseButton::Right, modifiers);
        cx.simulate_mouse_up(at, MouseButton::Right, modifiers);
        cx.run_until_parked();
    };
    let menu_open = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_, cx| view.read(cx).context_menu.is_open())
    };
    let terminal_focused = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|window, cx| view.read(cx).focus_handle.is_focused(window))
    };
    let take = |typed: &Arc<std::sync::Mutex<Vec<u8>>>| std::mem::take(&mut *typed.lock().unwrap());
    let handle = view.read_with(&cx, |view, _| view.focus_handle.clone());
    cx.update(|window, cx| handle.focus(window, cx));
    feed.output(b"hello");
    view.update(&mut cx, |view, _| {
        view.session.select_all();
    });

    // The menu takes the keyboard, keeps the selection, and its keys don't reach the program.
    right_click(&mut cx, Modifiers::default());
    assert!(menu_open(&mut cx));
    assert!(!terminal_focused(&mut cx));
    assert_eq!(
        view.read_with(&cx, |view, _| view.session.selection_text())
            .as_deref(),
        Some("hello")
    );
    cx.simulate_keystrokes("x");
    assert_eq!(take(&typed), b"");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!menu_open(&mut cx));
    assert!(terminal_focused(&mut cx), "Esc gives the keyboard back");

    // Hiding the tab closes it.
    right_click(&mut cx, Modifiers::default());
    view.update(&mut cx, |view, _| view.hide());
    assert!(!menu_open(&mut cx));
    view.update(&mut cx, |view, cx| view.show(cx));

    // A program tracking the mouse gets the plain right button; Shift opens the menu.
    feed.output(b"\x1b[?1000h");
    right_click(&mut cx, Modifiers::default());
    assert!(!menu_open(&mut cx));
    assert!(
        !take(&typed).is_empty(),
        "the press and release are reported"
    );
    right_click(&mut cx, Modifiers::shift());
    assert!(menu_open(&mut cx));
    assert_eq!(take(&typed), b"");

    // Paste asks about lines. The first Down lands on the disabled Copy (gpui-component
    // starts at index 0), whose Enter would do nothing without a selection.
    view.update(&mut cx, |view, _| view.session.clear_selection());
    cx.simulate_keystrokes("escape");
    right_click(&mut cx, Modifiers::shift());
    cx.update(|_, cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("a\nb\n".into())));
    cx.simulate_keystrokes("down down enter");
    cx.run_until_parked();
    assert!(!menu_open(&mut cx));
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "the paste question"
    );
    assert_eq!(take(&typed), b"");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(take(&typed), b"a\rb\r");
    assert!(terminal_focused(&mut cx));
}

#[gpui::test]
fn clearing_the_scrollback_drops_its_search_matches_at_once(cx: &mut gpui_kit::TestAppContext) {
    let spec = LaunchSpec::new(SessionKind::Shell, Some("sh".into()), Vec::new(), None);
    let (session, feed) = Session::remote(
        1,
        SessionKind::Shell,
        TermDimensions::new(20, 3),
        Palette::default(),
        Arc::new(|_| {}),
        Box::new(NoLink),
    );
    let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
    feed.output(b"x1\r\nx2\r\nx3\r\nx4\r\nx5");
    view.update(cx, |view, cx| {
        view.search.set_query("x").unwrap();
        view.search.refresh_now(&mut view.session.lock_term());
        assert_eq!(view.search.matches().len(), 5);
        view.session.scroll_display(Scroll::Top);
        view.clear_scrollback(cx);
        assert_eq!(view.search.matches().len(), 3);
        assert_eq!(view.session.display_offset(), 0);
    });
}

#[gpui::test]
fn the_menu_on_a_url_copies_the_link(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::Root;
    use gpui_kit::{Modifiers, MouseButton, point, px};

    cx.update(|cx| {
        gpui_kit::init(cx);
        settings_file::init_in_memory(cx);
        crate::terminal_element::init(cx);
    });
    let spec = LaunchSpec::new(SessionKind::Shell, Some("sh".into()), Vec::new(), None);
    let (session, feed) = Session::remote(
        1,
        SessionKind::Shell,
        TermDimensions::new(40, 5),
        Palette::default(),
        Arc::new(|_| {}),
        Box::new(NoLink),
    );
    let view = cx.new(|cx| TerminalView::from_test_session(session, &spec, cx));
    let root = view.clone();
    let window = cx.add_window(move |window, cx| Root::new(root, window, cx));
    let mut cx = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    let url = "https://example.com/menu";
    feed.output(format!("{url}\r\n{url}\r\n{url}").as_bytes());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let at = point(px(20.0), px(20.0));
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    assert!(cx.update(|_, cx| view.read(cx).context_menu.is_open()));
    // Open link, then Copy link.
    cx.simulate_keystrokes("down down enter");
    cx.run_until_parked();
    let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some(url));
}
