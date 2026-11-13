// No console window behind the app on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// Long test scenarios are exempt from the function length limit (docs/code-quality.md).
#![cfg_attr(test, allow(clippy::too_many_lines))]

mod about;
mod assets;
mod color;
mod file_explorer;
mod file_viewer;
mod folder_watcher;
mod git_page;
mod history;
mod history_popup;
mod hooks;
mod instance_lock;
mod keymap;
mod keys;
mod launcher;
mod light_theme;
mod mouse;
mod open_menu;
mod processes;
mod remote_hosts;
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
mod terminal_size;
mod terminal_view;
mod ui_dimensions;
mod window_background;
mod window_bounds;
mod workspace_view;

use std::path::PathBuf;
use std::rc::Rc;

use agent_status::{AGENT_ENV, PIPE_ENV, SESSION_ENV};
use futures::channel::mpsc;
use gpui_kit::component::TitleBar;
use gpui_kit::{App, AppContext, Context, Window, WindowDecorations, WindowOptions};
use term_core::{LaunchSpec, SessionKind};
use workspace_model::{AgentId, HOOK_PROGRAM_ENV, StartError, agents, folder_fallback};

use crate::hooks::AgentHooks;
use crate::instance_lock::InstanceLock;
use crate::workspace_view::{TabOwnership, WorkspaceView};

/// Usage: `agent-workspace [--remote <host>] [shell|claude|copilot] [project-dir]`. `None`
/// without arguments; bad arguments are printed and returned as the error the window shows.
fn launch_spec_from_args() -> Result<Option<LaunchSpec>, StartError> {
    parse_launch_args(std::env::args().skip(1), || std::env::current_dir().ok()).map_err(
        |message| {
            eprintln!("agent-workspace: {message}\n{}", usage());
            StartError::InvalidArguments {
                message,
                usage: usage(),
            }
        },
    )
}

/// The launch the arguments (without the program name) ask for; `current_dir` is the folder
/// when none is given. `--remote <host>` runs it on that ssh host (docs/remote.md), a shell
/// by default, in the given folder there or the server's home folder.
fn parse_launch_args(
    args: impl IntoIterator<Item = String>,
    current_dir: impl FnOnce() -> Option<PathBuf>,
) -> Result<Option<LaunchSpec>, String> {
    let mut args = args.into_iter().peekable();
    let host = if args.next_if(|arg| arg == "--remote").is_some() {
        match args.next() {
            Some(host) if !host.trim().is_empty() => Some(host),
            _ => return Err("--remote needs a host".into()),
        }
    } else {
        None
    };
    let kind = match args.next() {
        Some(kind) => kind,
        None if host.is_some() => "shell".into(),
        None => return Ok(None),
    };
    let folder = args.next().map(PathBuf::from);
    let folder = if host.is_some() {
        folder
    } else {
        folder.or_else(current_dir)
    };
    let mut launch = if kind == "shell" {
        LaunchSpec::shell(folder)
    } else if let Some(agent) = AgentId::from_cli_selector(&kind) {
        LaunchSpec::agent(agent, folder)
    } else {
        return Err(format!("unknown session kind {kind:?}"));
    };
    launch.host = host;
    Ok(Some(launch))
}

fn scrub_inherited_agent_env() {
    let inherited_markers = agents()
        .flat_map(|agent| agent.inherited_markers().iter().copied())
        .chain([AGENT_ENV, PIPE_ENV, SESSION_ENV, HOOK_PROGRAM_ENV]);
    for name in inherited_markers {
        // SAFETY: called first in `main`, before any other thread exists.
        unsafe { std::env::remove_var(name) };
    }
}

fn usage() -> String {
    let selectors = std::iter::once("shell".to_owned())
        .chain(agents().map(|agent| agent.cli_selector().to_owned()))
        .collect::<Vec<_>>()
        .join("|");
    format!("usage: agent-workspace [--remote <host>] [{selectors}] [project-dir]")
}

/// A requested folder on this machine that is not a folder: the tab opens in the home folder
/// instead. Returns the warning naming it. Checked here, before the UI thread runs.
fn replace_missing_folder(requested: Option<&mut LaunchSpec>) -> Option<String> {
    let spec = requested.filter(|spec| spec.host.is_none())?;
    let folder = spec.cwd.as_ref().filter(|folder| !folder.is_dir())?;
    eprintln!("agent-workspace: not a directory: {}", folder.display());
    let (fallback, warning) = folder_fallback(folder, std::env::home_dir());
    spec.cwd = fallback;
    Some(warning)
}

type HookMessages = mpsc::UnboundedReceiver<agent_status::HookMessage>;

/// The hooks, their messages, and the warning to show when they could not start.
fn start_hooks() -> (Option<Rc<AgentHooks>>, HookMessages, Option<String>) {
    let (hook_sender, hook_messages) = mpsc::unbounded();
    match AgentHooks::start(move |message| {
        let _ = hook_sender.unbounded_send(message);
    }) {
        Ok(hooks) => (Some(Rc::new(hooks)), hook_messages, None),
        Err(err) => {
            eprintln!("agent-workspace: agent status hooks disabled: {err}");
            (None, hook_messages, Some(err.warning()))
        }
    }
}

struct WorkspaceStartup {
    launchers: Rc<[launcher::Launcher]>,
    hooks: Option<Rc<AgentHooks>>,
    hook_messages: HookMessages,
    /// Shown once at start when the hooks could not start.
    hooks_warning: Option<String>,
    /// Shown once at start when the requested folder is not a folder.
    folder_warning: Option<String>,
    ownership: TabOwnership,
    tabs_writer: Rc<tabs_file::TabsWriter>,
    requested: Result<Option<LaunchSpec>, StartError>,
}

fn main() {
    scrub_inherited_agent_env();
    let mut requested = launch_spec_from_args();
    let folder_warning = replace_missing_folder(requested.as_mut().ok().and_then(Option::as_mut));
    let launchers: Rc<[launcher::Launcher]> = launcher::discover().into();
    let (hooks, hook_messages, hooks_warning) = start_hooks();
    // Only the first running instance reopens and saves the tabs; a later one opens just the
    // requested tab (or a shell), so it neither duplicates the sessions nor overwrites the save.
    let lock = InstanceLock::acquire();
    let ownership = if lock.is_some() {
        TabOwnership::Owner
    } else {
        TabOwnership::Guest
    };
    let tabs_writer = Rc::new(tabs_file::TabsWriter::new(ownership == TabOwnership::Owner));

    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            settings_file::init(cx);
            terminal_element::init(cx);
            terminal_view::init(cx);
            workspace_view::init(cx);
            settings_page::init_keyboard(cx);
            history::init(cx);
            cx.set_global(launcher::Launchers(launchers.clone()));
            let quit_hooks = hooks.clone();
            let closing_writer = tabs_writer.clone();
            cx.on_window_closed(move |cx, _| {
                if cx.windows().is_empty() {
                    closing_writer.finish();
                    history::finish(cx);
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
                    hooks_warning,
                    folder_warning,
                    ownership,
                    tabs_writer,
                    requested,
                },
                cx,
            );
            cx.activate(true);
        });
}

fn open_workspace_window(startup: WorkspaceStartup, cx: &mut App) {
    terminal_size::init(cx);
    // Client decorations work on Wayland without server-side decorations, too.
    let options = WindowOptions {
        window_bounds: Some(window_bounds::initial(cx)),
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some("agent-workspace".into()),
        window_background: window_background::initial(cx),
        ..TitleBar::window_options()
    };
    let shell_setting = settings_file::settings(cx)
        .default_shell()
        .map(str::to_owned);
    let requested = startup.requested.map(|requested| {
        requested.map(|spec| {
            // A remote shell is the server's default one.
            if spec.kind == SessionKind::Shell && spec.program.is_none() && spec.host.is_none() {
                return launcher::startup_shell(
                    &startup.launchers,
                    shell_setting.as_deref(),
                    spec.cwd,
                );
            }
            spec
        })
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
    let (window, _) = gpui_kit::open_window(options, cx, move |window, cx| {
        let workspace = cx.new(|cx| {
            WorkspaceView::new(
                startup.launchers,
                startup.hooks,
                startup.hook_messages,
                startup.tabs_writer,
                window,
                cx,
            )
        });
        workspace.update(cx, |workspace, cx| {
            // Before the tabs, so it stacks above a fallback-shell warning.
            if let Some(warning) = startup.hooks_warning {
                workspace.warn_agent_status_off(warning, window, cx);
            }
            if let Some(warning) = startup.folder_warning {
                workspace.warn_until_dismissed(warning, window, cx);
            }
            if startup.ownership == TabOwnership::Guest {
                workspace.note_guest_window(window, cx);
            }
            restore_or_open_first_tab(workspace, initial, window, cx);
        });
        let closing = workspace.downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            if let Some(workspace) = closing.upgrade() {
                // Busy tabs: the question's Close button closes the window instead.
                if !workspace.update(cx, |workspace, cx| workspace.allow_close_or_ask(window, cx)) {
                    return false;
                }
                workspace.read(cx).save_tabs_and_flush(cx);
            }
            window_bounds::save(window.window_bounds(), cx);
            true
        });
        workspace
    })
    .expect("failed to open window");
    window_background::follow_settings(window, cx);
}

struct InitialTabs {
    ownership: TabOwnership,
    /// The tab the command line asks for, or why it could not be read.
    requested: Result<Option<LaunchSpec>, StartError>,
    default_shell: LaunchSpec,
}

/// Restore the saved tabs, then open the requested tab, else the default shell when no tab
/// was restored; bad arguments show in the empty state, or in a toast beside restored tabs. A
/// remote tab opens after a local one, once connected: when its host cannot be reached, the
/// window keeps a tab and a notification says why.
fn restore_or_open_first_tab(
    workspace: &mut WorkspaceView,
    initial: InitialTabs,
    window: &mut Window,
    cx: &mut Context<WorkspaceView>,
) {
    let restored = initial.ownership == TabOwnership::Owner
        && tabs_file::load(settings_file::workspace_features(cx))
            .is_some_and(|state| workspace.restore(state, window, cx));
    let (requested, remote) = match initial.requested {
        Ok(Some(spec)) if spec.host.is_some() => (Ok(None), Some(spec)),
        requested => (requested, None),
    };
    match (requested, restored) {
        (Err(error), true) => workspace.warn_start_error(&error, window, cx),
        (Err(error), false) => workspace.show_start_failure(error, None, window, cx),
        (Ok(Some(spec)), true) => workspace.open_requested_tab(spec, window, cx),
        (Ok(None), true) => {}
        (Ok(requested), false) => {
            let spec = requested.unwrap_or(initial.default_shell);
            workspace.open_first_tab(spec, window, cx);
        }
    }
    if let Some(spec) = remote {
        // Deferred: the notification layer exists once the window is built.
        cx.defer_in(window, move |workspace, window, cx| {
            workspace.open_requested_tab(spec, window, cx);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_model::agents::{CLAUDE, COPILOT};

    fn parse(args: &[&str]) -> Result<Option<LaunchSpec>, String> {
        parse_launch_args(args.iter().map(|arg| arg.to_string()), || {
            Some(PathBuf::from("/cwd"))
        })
    }

    #[test]
    fn launch_args_name_a_kind_then_a_folder() {
        assert!(parse(&[]).unwrap().is_none());
        let claude = parse(&["claude", "/project"]).unwrap().unwrap();
        assert_eq!(claude.kind, SessionKind::Agent(CLAUDE));
        assert_eq!(claude.cwd, Some(PathBuf::from("/project")));
        let copilot = parse(&["copilot"]).unwrap().unwrap();
        assert_eq!(copilot.kind, SessionKind::Agent(COPILOT));
        assert_eq!(copilot.cwd, Some(PathBuf::from("/cwd")));
        assert_eq!(parse(&["shell"]).unwrap().unwrap().kind, SessionKind::Shell);
    }

    #[test]
    fn remote_launches_name_a_host_and_keep_the_folder_remote() {
        let shell = parse(&["--remote", "me@box"]).unwrap().unwrap();
        assert_eq!(shell.kind, SessionKind::Shell);
        assert_eq!(shell.host.as_deref(), Some("me@box"));
        // The server's home folder, not this machine's current one.
        assert_eq!(shell.cwd, None);
        let claude = parse(&["--remote", "box", "claude", "/srv/app"])
            .unwrap()
            .unwrap();
        assert_eq!(claude.kind, SessionKind::Agent(CLAUDE));
        assert_eq!(claude.cwd, Some(PathBuf::from("/srv/app")));
        assert_eq!(parse(&["shell"]).unwrap().unwrap().host, None);
        for args in [&["--remote"][..], &["--remote", " "]] {
            assert!(parse(args).unwrap_err().contains("host"));
        }
    }

    #[test]
    fn unknown_launch_kind_is_refused() {
        // A typo, or a folder given without a kind, must not open a shell somewhere else.
        for kind in ["cluade", "/some/project", ""] {
            let error = parse(&[kind]).unwrap_err();
            assert!(error.contains(&format!("{kind:?}")), "{error}");
        }
    }

    #[test]
    fn a_requested_folder_that_is_not_one_opens_in_the_home_folder() {
        let mut missing = parse(&["claude", "/nonexistent/agentws-missing"])
            .unwrap()
            .unwrap();
        let warning = replace_missing_folder(Some(&mut missing)).expect("a warning");
        assert!(
            warning.contains("/nonexistent/agentws-missing"),
            "{warning}"
        );
        assert_eq!(missing.cwd, std::env::home_dir());

        let folder = std::env::temp_dir();
        let mut existing = parse(&["shell", folder.to_str().unwrap()])
            .unwrap()
            .unwrap();
        assert_eq!(replace_missing_folder(Some(&mut existing)), None);
        assert_eq!(existing.cwd, Some(folder));

        // A remote folder is checked on the server.
        let mut remote = parse(&["--remote", "box", "claude", "/nonexistent/agentws-missing"])
            .unwrap()
            .unwrap();
        assert_eq!(replace_missing_folder(Some(&mut remote)), None);
        assert_eq!(
            remote.cwd,
            Some(PathBuf::from("/nonexistent/agentws-missing"))
        );
    }
}
