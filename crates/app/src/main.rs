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
mod window_bounds;
mod workspace_view;

use std::path::PathBuf;
use std::process;
use std::rc::Rc;

use futures::channel::mpsc;
use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::{AppContext, WindowDecorations, WindowOptions};
use term_core::LaunchSpec;

use crate::hooks::ClaudeHooks;
use crate::instance_lock::InstanceLock;
use crate::workspace_view::WorkspaceView;

/// Usage: `agent-workspace [shell|claude|copilot] [project-dir]`. `None` without arguments.
fn launch_spec_from_args() -> Option<LaunchSpec> {
    let mut args = std::env::args().skip(1);
    let kind = args.next()?;
    let cwd = args
        .next()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    Some(match kind.as_str() {
        "claude" => LaunchSpec::claude_code(cwd),
        "copilot" => LaunchSpec::copilot_cli(cwd),
        _ => LaunchSpec::shell(cwd),
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

fn main() {
    for name in INHERITED_AGENT_MARKERS {
        // SAFETY: runs first in `main`, before any other thread exists.
        unsafe { std::env::remove_var(name) };
    }
    let requested = launch_spec_from_args();
    if let Some(cwd) = requested
        .as_ref()
        .and_then(|spec| spec.cwd.as_ref())
        .filter(|cwd| !cwd.is_dir())
    {
        eprintln!("agent-workspace: not a directory: {}", cwd.display());
        process::exit(2);
    }

    let launchers: Rc<[launcher::Launcher]> = launcher::discover().into();
    let (hook_sender, hook_messages) = mpsc::unbounded();
    let hooks = match ClaudeHooks::start(move |message| {
        let _ = hook_sender.unbounded_send(message);
    }) {
        Ok(hooks) => Some(Rc::new(hooks)),
        Err(err) => {
            eprintln!("agent-workspace: Claude Code status disabled: {err}");
            None
        }
    };

    // Only the first running instance reopens and saves the tabs; a later one opens just the
    // requested tab (or a shell), so it neither duplicates the sessions nor overwrites the save.
    let lock = InstanceLock::acquire();
    let owns_tabs = lock.is_some();

    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            settings_file::init(cx);
            terminal_view::init(cx);
            workspace_view::init(cx);
            history::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
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

            // The app draws its own title bar and frame: GNOME on Wayland offers no server-side
            // decorations, and a drawn title bar looks the same on every platform.
            let options = WindowOptions {
                window_bounds: Some(window_bounds::initial(cx)),
                window_decorations: Some(WindowDecorations::Client),
                // Must match the .desktop file name, or GNOME shows a generic icon.
                app_id: Some("agent-workspace".into()),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, move |window, cx| {
                let workspace = cx.new(|cx| {
                    WorkspaceView::new(launchers, hooks, hook_messages, owns_tabs, window, cx)
                });
                // The tabs open at the last close come back; a tab named on the command line
                // opens after them, at the top level. Without either, a shell opens in the
                // current folder.
                workspace.update(cx, |workspace, cx| {
                    let restored = owns_tabs
                        && tabs_file::load()
                            .is_some_and(|state| workspace.restore(state, window, cx));
                    if restored {
                        if let Some(spec) = requested.clone() {
                            workspace.open_requested_tab(spec, window, cx);
                        }
                        return;
                    }
                    let spec = requested
                        .clone()
                        .unwrap_or_else(|| LaunchSpec::shell(std::env::current_dir().ok()));
                    if let Err(err) = workspace.open_tab(spec.clone(), window, cx) {
                        let program = launcher::launch_name(&spec);
                        eprintln!("agent-workspace: failed to start {program}: {err}");
                        process::exit(1)
                    }
                });
                // Closed by the window manager (its close button, Alt+F4) or, on Windows and
                // macOS, by the title bar's close button. Closing the last tab and the Linux
                // title bar button go through `WorkspaceView::close_window`.
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
            cx.activate(true);
        });
}
