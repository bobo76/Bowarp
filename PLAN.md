# Agent Workspace - Implementation Plan

Feature spec: [FEATURES.md](FEATURES.md)

## Status (2026-10-18)

What is done is described in [FEATURES.md](FEATURES.md); the details are in the git history.

**Next steps, in priority order**

1. Automatic version increments: a `scripts/bump-version.sh [patch|minor|major]` (default
   `patch`) raises the workspace `version` in `Cargo.toml` (`0.1.0` -> `0.1.1`; `minor` and
   `major` reset the lower parts), refreshes `Cargo.lock` (`cargo update -w`) and prints the
   new version; it refuses a dirty `Cargo.toml` and a version that isn't `X.Y.Z`. The
   pipeline's squash step (`PIPELINE-PLAN.md`) runs it before the squashed commit, so every
   pushed feature is one patch higher, then tags the commit `v<version>` and pushes the tag
   with the branch. Finishing a milestone bumps `minor` instead (M2 done -> `0.2.0`). The
   About dialog already shows `CARGO_PKG_VERSION` plus the commit, so nothing changes there.
   Check: the script on a scratch copy (patch, minor, major, dirty file, bad version),
   `cargo build` picks up the new version, and the About line shows it.

**Not doable yet**

- Windows: "Bash" opened Ubuntu (Git Bash / WSL distro launchers): code done in `launcher.rs`, needs a Windows machine to build and run (this machine is Linux).
- Windows: a tab's working directory (`NtQueryInformationProcess` PEB read); until then it
  shows the launch folder.
- Windows: the hover card's `git` child may need `CREATE_NO_WINDOW` (untested).
- Windows: check that the title bar buttons (sidebar, explorer, About) click. They were dead
  because gpui-component's `TitleBar` marks its child area `WindowControlArea::Drag`, so
  `WM_NCHITTEST` answered `HTCAPTION` over them and `DefWindowProc`'s move loop ate the
  mouse-up; they now sit in a `div().occlude()` wrapper, which should give them `HTCLIENT`.

**Deferred (needs a Windows machine):** Windows build and the same check (ConPTY, named pipe,
hook command quoting).

## Decisions

- **Stack:** Rust + GPUI + `alacritty_terminal`.
  - Priorities: performance, then memory, then Windows, then Ubuntu.
  - Native GPU rendering, no browser engine, event-driven redraw (zero work when idle).
- **Development platform:** start on Ubuntu; same code targets Windows (ConPTY) with regular Windows testing.
- **Agent status scope for M1:** Claude Code only. Copilot CLI and plain shells show
  "recent activity" based on output timestamps.
- **Licensing:** Zed's `terminal` / `terminal_view` crates are GPL-3 - reference only, do not copy.
  `gpui` and `alacritty_terminal` are Apache-2.0.

## Crates (versions as of 2026-10-16)

| Need | Crate | Note |
|---|---|---|
| UI + widgets | `gpui-kit` 0.7.3 | one crate: pins GPUI (`gpui-pre` 0.3.9) and re-exports it with `gpui-component` |
| Terminal emu + pty | `alacritty_terminal` 0.26 | built-in `tty` module (ConPTY on Windows) + `EventLoop` |
| Hook IPC | `interprocess` 2.4 | named pipe (Windows) / Unix socket (Linux) |
| Persistence | `serde`, `serde_json`, `directories` | |
| Sound (M5) | `rodio` | |

## Workspace Layout

```
agent-workspace/
├── crates/
│   ├── term-core/        # session = pty + alacritty Term + event listener (UI-agnostic, testable)
│   ├── agent-status/     # status state machine + IPC server for hook events
│   ├── workspace-model/  # UI-free logic: tab location, layout, persistence, file tree/viewer, history
│   ├── agentws-hook/     # tiny binary invoked by Claude Code hooks
│   └── app/              # GPUI UI: sidebar, terminal element, panes
```

Only `app` depends on GPUI. If GPUI proves too painful, only that crate is replaced (iced/egui).

## Target UI

Reference screenshots: [sidebar + hover card](docs/ui-reference/sidebar-hover-card.png),
[tab context menu](docs/ui-reference/tab-context-menu.png). Still to do from them:

| Element | Look | Milestone |
|---|---|---|
| Title bar | settings button once there are settings | - |
| Context menu | Save as new config (= save as a named workspace) | M4 |
| Clipboard notice | banner above the terminal when a program writes the clipboard (OSC 52), with allow/deny | M5 |

"Share session" from the screenshot is not planned.

## Later Milestones

- **M3:** split panes, adjustable sizes, pane shortcuts.
- **M4:** named workspaces (the single window's tabs are already restored), offer to relaunch; restored tabs clearly marked "not running".
- **M5:** terminal search, close-with-running-process warnings, notifications + configurable sound.
- **M6:** Copilot CLI status (if it gains a hook mechanism), Windows packaging, Ubuntu polish
  (Wayland/X11, socket paths).

## Backlog

- Draw box-drawing and block characters procedurally; font glyphs leave small gaps between
  rows for heavy/double verticals (`┃`, `║`).
- Report drags that leave the terminal area (GPUI only delivers moves while hovered).
- Answer OSC 52 clipboard requests.
- Show an "exited" banner in the terminal area (currently only in the title).
- Tune wheel scroll speed (about 2 lines per notch on X11).
- Configurable font family and size.
- Configurable default shell for the Terminal entry and Ctrl+Shift+T, saved in the settings
  file (overrides the $SHELL / Git Bash default).
- Configurable keyboard shortcuts: load a user keymap (keystroke -> action name, built with
  `cx.build_action`) in place of `default_bindings`.
- Interrupting Claude Code (Esc) leaves the tab Working until the `idle_prompt` notification
  (~60 s); no hook fires on interrupt.
- New-tab folder picker (currently the selected tab's folder).
- Drag group headers to reorder groups (the group menu's Move up / down does it for now).
- Sidebar search: Up/Down to move through the matches from the search field; the filter/sort
  button from the reference screenshot.
- Icon overrides per tab (tab menu); what can be picked (bundled icons, emoji, letters) is
  still to decide.
- Working directory from OSC 7 (only useful for remote shells): `alacritty_terminal` does not
  parse it, but the pty reader wrapper (`term-core/src/prompt.rs`) could find it.
- File explorer: read folders off the UI thread (a "Loading..." row, results applied per
  root) so a folder with tens of thousands of entries or a slow network mount doesn't stall
  the window; Delete already runs in the background.
- File viewer: keep the scroll position when the file is reloaded after a change on disk
  (it goes back to the top); open links to other files (`FEATURES.md`) in the viewer instead
  of the system handler; drag the viewer's left edge to resize it (fixed at 45% of the
  width); the `⋮` menu from the reference screenshot.
- Split panes (M3), in three steps:
  - Split panes: split the selected tab's area right or down (tab menu entries and a
    shortcut each) into panes, each running its own session started in the focused pane's
    folder; a click focuses a pane (focused pane outlined), closing a pane's session or its
    `×` removes it and the remaining pane takes the space. The tab row shows the focused
    pane's title, status and folder; the tab's status is the most urgent of its panes. Panes
    nest (a split inside a split). Pure layout tree (split, remove, focus order) in
    `workspace-model` with unit tests.
  - Adjustable sizes and restore: drag the divider between two panes to resize them
    (minimum size per pane, double-click resets to an even split); the pane layout and sizes
    are saved in `tabs.json` and restored with the tab.
  - Pane shortcuts: move focus to the pane left / right / above / below, close the focused
    pane, zoom the focused pane to the whole tab and back; hints in the tab menu and
    README.md.
- Untested on Windows: hook command quoting (`"C:\...\agentws-hook.exe"` run by Claude Code's
  shell) and `.cmd` launchers (`claude.cmd` from npm) under ConPTY.

## Risks

- **GPUI API churn:** exact `gpui-pre` version; UI isolated in `app`.
- **IME and clipboard edge cases** in a custom terminal element: test early in M1, especially on Windows.
- **Claude Code `--settings` flag and hook payload shapes:** verify against current docs before relying on them.

## Ubuntu Prerequisites

- Rust toolchain via `rustup`.
- GPUI build deps (Vulkan, X11/Wayland, fonts):

  ```
  sudo apt install build-essential pkg-config libvulkan-dev libwayland-dev libxkbcommon-x11-dev \
    libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev libssl-dev clang cmake
  ```
