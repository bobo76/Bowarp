# Agent Workspace - Implementation Plan

Feature spec: [FEATURES.md](FEATURES.md)

## Status (2026-10-16)

**Done so far**

- Stack chosen (Rust + GPUI + `alacritty_terminal`), project scaffolded on Ubuntu, git initialized.
- M1 step 1 - `term-core`: pty sessions, coalesced wakeups, exit after output drain, palette with
  OSC 10/11 color replies. Committed (`5d39cfa`).
- M1 step 2 - `app`: GPUI window rendering one session (shell or Claude Code) with keyboard,
  IME, selection, copy/paste, resize, drawn title bar and window frame. Committed with the
  scroll fix and palette.
- Scroll fix: Claude Code (2.1.302) runs on the alternate screen with mouse tracking
  (`?1049h`, `?1000/1002/1003h`, SGR `?1006h`), so the wheel had no scrollback to move and
  sent nothing. Wheel, clicks and motion are now forwarded as xterm mouse reports when a
  program enables tracking; Shift bypasses reporting for local selection (`app/src/mouse.rs`).
  Verified under X11 (Xvfb): shell scrollback, Claude Code transcript scrolling, and exact
  report bytes. Scrolling confirmed on Wayland by the user.
- Scroll bar: overlay in the right padding (no column cost), shown while scrolling, hovered or
  dragged, hidden after 1.2 s and whenever there is no history (alternate screen). Drag the thumb
  (keeps tracking outside the window), click the track to center the thumb there
  (`app/src/scrollbar.rs`, `Session::scroll_to`). Verified under X11 (Xvfb).
- M1 step 3 - sidebar: tab list with session-type icon, name and status dot; `+` menu with the
  shells found on PATH (default shell first), Claude Code and Copilot CLI; tab shortcuts.
  Verified under X11 (Xvfb).
- M1 step 4 - Claude Code status via hooks (`agent-status`, `agentws-hook`). Verified under X11
  with Claude Code 2.1.302: Working, Needs attention (permission prompt) and Idle shown on an
  unselected tab; activity dot on a background shell.
- Tests: 73 passing (`term-core` session + palette, `agent-status` state machine + IPC,
  `agentws-hook` round trip with the real binary, `app` key/paste translation, mouse reports,
  wheel decision, render batching/colors, scroll bar geometry, launcher discovery).
- Verified by driving the app under X11 (screenshots + simulated input). Wayland checked
  manually by the user.

**Next steps, in priority order**

1. M1 "done when" check: 3 Claude Code sessions + 1 shell on Ubuntu (Wayland, by the user).
2. Windows build and the same check (ConPTY, named pipe, hook command quoting).

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
│   ├── workspace-model/  # groups/tabs/colors/layout + JSON persistence (M2+)
│   ├── agentws-hook/     # tiny binary invoked by Claude Code hooks
│   └── app/              # GPUI UI: sidebar, terminal element, panes
```

Only `app` depends on GPUI. If GPUI proves too painful, only that crate is replaced (iced/egui).

## Milestone 1 - One Window, Real Sessions, Real Status

1. **Session core (`term-core`)** - done
   - Spawn shell / `claude` / `copilot` in a chosen folder via `alacritty_terminal::tty`.
   - One I/O thread per session.
   - Child env sets `TERM`/`COLORTERM` per session (alacritty's `tty::setup_env` mutates the
     process env and is not thread-safe).
   - `Exited` is reported on alacritty's `Exit` (after the final output drain), preceded by a wakeup.
   - Coalesce wakeups to at most one repaint per frame; no work while idle.
2. **Terminal rendering (`app`)** - done
   - Custom GPUI element painting the visible grid: cells, colors, cursor, selection.
   - Keyboard input (incl. Ctrl-C, multiline), mouse scrollback, copy/paste.
   - Resize propagated to both the pty and `Term`.
   - Terminal lock held only to copy visible cells; shaping and painting happen after release.
   - ASCII runs are batched per style; other characters are placed at their own column so wide,
     fallback-font, and combining glyphs stay on the grid.
   - IME via `EntityInputHandler`; special keys via `on_key_down` (`keys.rs`, unit-tested).
   - The window root binds Tab, Shift+Tab, Ctrl+C; the `Terminal` key context overrides them
     with `NoAction` so they reach the program.
   - Copy/paste: Ctrl+Shift+C/V, Shift+Insert; Linux primary selection on select + middle click.
   - Client-side decorations with `gpui-component`'s `TitleBar` (title, min/max/close, drag,
     double-click to maximize) and window border; GNOME on Wayland has no server-side
     decorations. The bundled icon assets must be registered for the control buttons to render.
   - Programs' OSC 10/11 color queries are answered from the palette (`term-core/palette.rs`).
   - Inherited Claude Code session markers (`CLAUDECODE`, `CLAUDE_CODE_CHILD_SESSION`, ...) are
     stripped at startup so sessions are not treated as nested children.
   - Overlay scroll bar for the scrollback, hidden on the alternate screen: the program scrolls
     its own content there and the terminal cannot know its position.
   - Mouse reporting (modes 1000/1002/1003, SGR/UTF-8/legacy encodings): wheel, clicks and
     motion (once per cell) go to programs that enable tracking; Shift keeps local selection
     and scrollback. Without tracking, the alternate screen gets arrow keys on wheel.
3. **Sidebar** - done
   - Flat tab list: session-type icon, name (program title, else kind/shell name), status dot,
     close button on the selected or hovered row (`sidebar.rs`).
   - `+` menu: shells found on PATH (default `$SHELL` first; PowerShell/cmd on Windows), Claude
     Code, Copilot CLI (shown disabled when not installed). New tabs open in the selected tab's
     folder (`launcher.rs`).
   - Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+1..9, Ctrl+Shift+T (another tab like the selected one),
     Ctrl+Shift+W (close; the last tab closes the window). Handled in the workspace's key
     capture phase so they never reach the terminal.
   - Only the selected terminal paints; hidden ones re-arm their own wakeups and emit
     `TabChanged` only when title/status/activity changes, so background output does not redraw
     the window.
4. **Agent status (Claude Code)** - done
   - Claude Code tabs get `AGENTWS_SESSION=<session id>` and `AGENTWS_PIPE=agentws-<pid>.sock`,
     and launch as `claude --settings <temp file>` (a file, not inline JSON, to avoid Windows
     command-line quoting). The file is removed when the last window closes. The user's
     `~/.claude/settings.json` is never modified; `--settings` hooks merge with the user's.
   - `agentws-hook` (built next to the app; without it status is disabled with a warning) reads
     the hook JSON, sends one line `<session> <start µs> <event> [notification_type]` over an
     `interprocess` local socket (Unix socket / named pipe) and always exits 0 silently.
   - The state machine (`agent-status`) orders events by hook start time, so a slow hook cannot
     overwrite a newer status; Exited is final.

     | Event | Status |
     |---|---|
     | `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionDenied` | Working |
     | `PermissionRequest`, `Notification` (`permission_prompt`, `elicitation_*dialog`, `agent_needs_input`) | Needs attention |
     | `Stop`, `StopFailure`, `Notification` (`idle_prompt`) | Idle |
     | `SessionEnd`, process exit | Exited |

     `PostToolUse` returns an approved permission prompt to Working. `Stop` does not fire when
     the user interrupts; the `idle_prompt` notification (about a minute later) returns it to Idle.
   - Sessions without hooks show "recent output" for 2 s after output, then "quiet".
5. **Tests**
   - `term-core`: spawn `echo`, assert grid content; resize propagation.
   - `agent-status`: all event sequences, including out-of-order events.
   - `agentws-hook`: pipe round-trip.
   - UI: manual verification.

**Done when:** 3 Claude Code sessions + 1 shell run on Ubuntu (then Windows) with smooth typing and resizing,
and unselected tabs show correct Working / Needs attention / Idle.

## Later Milestones

- **M2:** named collapsible groups, colors with tab inheritance, icon overrides, aggregate status on collapsed
  groups, "jump to next session needing attention".
- **M3:** split panes, adjustable sizes, pane shortcuts, move tabs between groups.
- **M4:** named workspaces, save/restore layout, offer to relaunch; restored tabs clearly marked "not running".
- **M5:** terminal search, close-with-running-process warnings, notifications + configurable sound.
- **M6:** Copilot CLI status (if it gains a hook mechanism), Windows packaging, Ubuntu polish
  (Wayland/X11, socket paths).

## Backlog (found during M1)

- Draw box-drawing and block characters procedurally; font glyphs leave small gaps between
  rows for heavy/double verticals (`┃`, `║`).
- Report drags that leave the terminal area (GPUI only delivers moves while hovered).
- Answer OSC 52 clipboard requests.
- Show an "exited" banner in the terminal area (currently only in the title).
- Tune wheel scroll speed (about 2 lines per notch on X11).
- Configurable font family and size.
- Interrupting Claude Code (Esc) leaves the tab Working until the `idle_prompt` notification
  (~60 s); no hook fires on interrupt.
- New-tab folder picker (currently the selected tab's folder).
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
