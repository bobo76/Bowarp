# Agent Workspace - Implementation Plan

Feature spec: [FEATURES.md](FEATURES.md)

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
| UI | `gpui` = `gpui-pre` 0.3.9 (published from the zed repo) | exact version required by `gpui-component`; no git pin needed |
| Widgets | `gpui-component` 0.7.3 | tabs, tree, resizable panes, icons |
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
│   ├── workspace-model/  # groups/tabs/colors/layout + JSON persistence
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
2. **Terminal rendering (`app`)**
   - Custom GPUI element painting the visible grid: cells, colors, cursor, selection.
   - Keyboard input (incl. Ctrl-C, multiline), mouse scrollback, copy/paste.
   - Resize propagated to both the pty and `Term`.
3. **Sidebar**
   - Flat tab list: session-type icon, name, status dot.
   - New/close tab, Ctrl+Tab and Ctrl+1..9.
4. **Agent status (Claude Code)**
   - On spawn, set `AGENTWS_SESSION=<uuid>` and `AGENTWS_PIPE=<pipe name>`.
   - Launch `claude --settings <generated json>` registering `agentws-hook` for
     `UserPromptSubmit`, `PreToolUse`, `Notification`, `Stop`, `SessionEnd`.
     The user's global `~/.claude/settings.json` is never modified.
   - `agentws-hook` reads hook JSON from stdin, forwards it with the session id over the pipe, exits.
     Missing pipe → silent `exit 0`.
   - State machine:

     | Event | Status |
     |---|---|
     | `UserPromptSubmit`, `PreToolUse` | Working |
     | `Notification` | Needs attention |
     | `Stop` | Idle |
     | `SessionEnd`, process exit | Exited |

   - Sessions without hooks show only "recent activity" (output timestamps).
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
