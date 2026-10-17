mod keys;
mod mouse;
mod terminal_element;
mod terminal_view;
mod workspace_view;

use std::path::PathBuf;
use std::process;

use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::{
    AppContext, Bounds, Focusable, WindowBounds, WindowDecorations, WindowOptions, px, size,
};
use term_core::LaunchSpec;

use crate::terminal_view::TerminalView;
use crate::workspace_view::WorkspaceView;

/// Usage: `agent-workspace [shell|claude|copilot] [project-dir]`
fn launch_spec_from_args() -> LaunchSpec {
    let mut args = std::env::args().skip(1);
    let kind = args.next();
    let cwd = args
        .next()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    match kind.as_deref() {
        Some("claude") => LaunchSpec::claude_code(cwd),
        Some("copilot") => LaunchSpec::copilot_cli(cwd),
        _ => LaunchSpec::shell(cwd),
    }
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
    let spec = launch_spec_from_args();
    if let Some(cwd) = spec.cwd.as_ref().filter(|cwd| !cwd.is_dir()) {
        eprintln!("agent-workspace: not a directory: {}", cwd.display());
        process::exit(2);
    }

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            terminal_view::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1000.), px(650.)), cx);
            // The app draws its own title bar and frame: GNOME on Wayland offers no server-side
            // decorations, and a drawn title bar looks the same on every platform.
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_decorations: Some(WindowDecorations::Client),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, move |window, cx| {
                let terminal = cx.new(|cx| {
                    TerminalView::new(&spec, cx).unwrap_or_else(|err| {
                        let program = spec.program.as_deref().unwrap_or("shell");
                        eprintln!("agent-workspace: failed to start {program}: {err}");
                        process::exit(1)
                    })
                });
                window.focus(&terminal.focus_handle(cx), cx);
                cx.new(|cx| WorkspaceView::new(terminal, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
