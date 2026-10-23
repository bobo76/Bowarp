// No console window behind the app on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod about;
mod assets;
mod file_explorer;
mod file_viewer;
mod folder_watcher;
mod history;
mod history_popup;
mod hooks;
mod instance_lock;
mod keys;
mod launcher;
mod mouse;
mod scrollbar;
mod settings_file;
mod settings_page;
mod shell_integration;
mod shortcuts;
mod sidebar;
mod tab_menu;
mod tabs_file;
mod terminal_element;
mod terminal_links;
mod terminal_view;
mod ui_dimensions;
mod window_bounds;
mod workspace_view;

use std::path::PathBuf;
use std::process;
use std::rc::Rc;

use futures::channel::mpsc;
use gpui_kit::component::TitleBar;
use gpui_kit::{App, AppContext, Context, Window, WindowDecorations, WindowOptions};
use term_core::{LaunchSpec, SessionKind};

use crate::hooks::ClaudeHooks;
use crate::instance_lock::InstanceLock;
use crate::workspace_view::{TabOwnership, WorkspaceView};

/// Usage: `agent-workspace [shell|claude|copilot] [project-dir]`. `None` without arguments.
fn launch_spec_from_args() -> Option<LaunchSpec> {
    let mut args = std::env::args().skip(1);
    let kind = args.next()?;
    let folder = args
        .next()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    Some(match kind.as_str() {
        "claude" => LaunchSpec::claude_code(folder),
        "copilot" => LaunchSpec::copilot_cli(folder),
        _ => LaunchSpec::shell(folder),
    })
}

/// Markers a running Claude Code sets on its children. When this app is started from inside a
/// Claude Code session they would leak into every session we spawn, making each one act as a
/// nested child (e.g. transcript saving disabled). User configuration such as
/// `CLAUDE_CONFIG_DIR` is deliberately kept.
const INHERITED_AGENT_MARKERS: &[&str] = &[
    "AI_AGENT",
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_VERSION",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
];

fn scrub_inherited_agent_env() {
    for name in INHERITED_AGENT_MARKERS {
        // SAFETY: called first in `main`, before any other thread exists.
        unsafe { std::env::remove_var(name) };
    }
}

fn validate_requested_folder(requested: Option<&LaunchSpec>) {
    if let Some(folder) = requested
        .and_then(|spec| spec.cwd.as_ref())
        .filter(|folder| !folder.is_dir())
    {
        eprintln!("agent-workspace: not a directory: {}", folder.display());
        process::exit(2);
    }
}

type HookMessages = mpsc::UnboundedReceiver<agent_status::HookMessage>;

fn start_hooks() -> (Option<Rc<ClaudeHooks>>, HookMessages) {
    let (hook_sender, hook_messages) = mpsc::unbounded();
    let hooks = match ClaudeHooks::start(move |message| {
        let _ = hook_sender.unbounded_send(message);
    }) {
        Ok(hooks) => Some(Rc::new(hooks)),
        Err(err) => {
            eprintln!("agent-workspace: agent hooks and Copilot mode bridge disabled: {err}");
            None
        }
    };
    (hooks, hook_messages)
}

struct WorkspaceStartup {
    launchers: Rc<[launcher::Launcher]>,
    hooks: Option<Rc<ClaudeHooks>>,
    hook_messages: HookMessages,
    ownership: TabOwnership,
    requested: Option<LaunchSpec>,
}

fn main() {
    scrub_inherited_agent_env();
    let requested = launch_spec_from_args();
    validate_requested_folder(requested.as_ref());
    let launchers = launcher::discover().into();
    let (hooks, hook_messages) = start_hooks();
    // Only the first running instance reopens and saves the tabs; a later one opens just the
    // requested tab (or a shell), so it neither duplicates the sessions nor overwrites the save.
    let lock = InstanceLock::acquire();
    let ownership = if lock.is_some() {
        TabOwnership::Owner
    } else {
        TabOwnership::Guest
    };

    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            settings_file::init(cx);
            terminal_element::init(cx);
            terminal_view::init(cx);
            workspace_view::init(cx);
            history::init(cx);
            let quit_hooks = hooks.clone();
            cx.on_window_closed(move |cx, _| {
                if cx.windows().is_empty() {
                    if let Some(hooks) = &quit_hooks {
                        hooks.remove_settings();
                    }
                    if let Some(lock) = &lock {
                        lock.release();
                    }
                    cx.quit();
                }
            })
            .detach();

            open_workspace_window(
                WorkspaceStartup {
                    launchers,
                    hooks,
                    hook_messages,
                    ownership,
                    requested,
                },
                cx,
            );
            cx.activate(true);
        });
}

fn open_workspace_window(startup: WorkspaceStartup, cx: &mut App) {
    // Client decorations work on Wayland without server-side decorations, too.
    let options = WindowOptions {
        window_bounds: Some(window_bounds::initial(cx)),
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some("agent-workspace".into()),
        ..TitleBar::window_options()
    };
    let shell_setting = settings_file::settings(cx)
        .default_shell()
        .map(str::to_owned);
    let requested = startup.requested.map(|spec| {
        if spec.kind == SessionKind::Shell && spec.program.is_none() {
            return launcher::startup_shell(&startup.launchers, shell_setting.as_deref(), spec.cwd);
        }
        spec
    });
    let initial = InitialTabs {
        ownership: startup.ownership,
        requested,
        default_shell: launcher::startup_shell(
            &startup.launchers,
            shell_setting.as_deref(),
            std::env::current_dir().ok(),
        ),
    };
    gpui_kit::open_window(options, cx, move |window, cx| {
        let workspace = cx.new(|cx| {
            WorkspaceView::new(
                startup.launchers,
                startup.hooks,
                startup.hook_messages,
                startup.ownership,
                window,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| {
            restore_or_open_first_tab(workspace, initial, window, cx);
        });
        let closing = workspace.downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            if let Some(workspace) = closing.upgrade() {
                workspace.read(cx).save_tabs(cx);
            }
            window_bounds::save(window.window_bounds());
            true
        });
        workspace
    })
    .expect("failed to open window");
}

struct InitialTabs {
    ownership: TabOwnership,
    requested: Option<LaunchSpec>,
    default_shell: LaunchSpec,
}

fn restore_or_open_first_tab(
    workspace: &mut WorkspaceView,
    initial: InitialTabs,
    window: &mut Window,
    cx: &mut Context<WorkspaceView>,
) {
    let restored = initial.ownership == TabOwnership::Owner
        && tabs_file::load().is_some_and(|state| workspace.restore(state, window, cx));
    if restored {
        if let Some(spec) = initial.requested {
            workspace.open_requested_tab(spec, window, cx);
        }
        return;
    }
    open_first_tab(
        workspace,
        initial.requested.unwrap_or(initial.default_shell),
        window,
        cx,
    );
}

fn open_first_tab(
    workspace: &mut WorkspaceView,
    spec: LaunchSpec,
    window: &mut Window,
    cx: &mut Context<WorkspaceView>,
) {
    // An installed but broken shell falls back to the platform's automatic shell.
    let fallback = (spec.kind == SessionKind::Shell && spec.program.is_some())
        .then(|| LaunchSpec::shell(spec.cwd.clone()));
    let mut program = launcher::launch_name(&spec).to_owned();
    let mut started = workspace.open_tab(spec, window, cx);
    if let (Err(err), Some(fallback)) = (&started, fallback) {
        eprintln!("agent-workspace: failed to start {program}: {err}");
        let failure = format!("Could not start {program}: {err}");
        program = launcher::launch_name(&fallback).to_owned();
        started = workspace.open_tab(fallback, window, cx);
        if started.is_ok() {
            workspace.warn_shell_fallback(failure, window, cx);
        }
    }
    if let Err(err) = started {
        eprintln!("agent-workspace: failed to start {program}: {err}");
        process::exit(1)
    }
}
