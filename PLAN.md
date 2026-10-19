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

- **M4:** named workspaces (the single window's tabs are already restored), offer to relaunch; restored tabs clearly marked "not running".
- **M5:** terminal search, close-with-running-process warnings, notifications + configurable sound.
- **M6:** Copilot CLI status (if it gains a hook mechanism), Windows packaging, Ubuntu polish
  (Wayland/X11, socket paths).

## Backlog

- Interrupting Claude Code (Esc) leaves the tab Working until the `idle_prompt` notification
  (~60 s); no hook fires on interrupt.
- Show an "exited" banner in the terminal area (currently only in the title).
- Tune wheel scroll speed (about 2 lines per notch on X11).
- Quitting an agent falls back to a shell: when a Claude Code / Copilot tab's program exits,
  the tab starts the default shell in the agent's last folder (its last known `location()`,
  since `current_dir` is `None` once the child exited) and becomes a plain shell tab: shell
  icon and status, saved as a shell in `tabs.json`. Today the tab is left dead as "Exited".
  The new terminal starts empty, so the agent's output is lost, the
  `claude --resume <id>` line included (decision in [README.md](README.md#design-decisions)).
- Agent icon in a shell tab: while `claude` or `copilot` runs from a shell tab's prompt, its
  sidebar row shows the Claude / Copilot icon, and the shell icon returns at the next prompt.
  The command comes from the shell integration's OSC 133 `C` mark (`on_executed` in
  `term-core/src/prompt.rs`, which command history already uses); its first word is matched
  by file name (path and `.exe` / `.cmd` stripped, like `shell_icon`). Needs shell
  integration, so it misses scripts, `npx` and aliases. Icon only: the status stays shell-like
  (no `--settings` hooks for a `claude` typed at the prompt).
- Clickable links in terminal output: Ctrl+click opens a URL in the browser, both plain-text
  URLs found in the visible grid and OSC 8 hyperlinks (`alacritty_terminal` keeps them on the
  cell). A `path`, `path:line` or `path:line:col` (`src/foo.rs:42`, which Claude Code prints
  all the time), resolved against the tab's folder and only if it exists, opens in the file
  viewer scrolled to that line. Underline and hand cursor while Ctrl is held over a link.
- Bell as an attention signal: a BEL (`SessionEvent::Bell`, ignored today in
  `terminal_view.rs`) in a tab that isn't selected marks it as needing attention until it
  is viewed, like a finished agent. This covers programs without hooks (Copilot, scripts).
  Optional sound with the M5 notification sound; rate-limited so a stream of bells doesn't
  repeat it.
- Long command finished in a shell tab: when a command that ran longer than a threshold
  (default 10 s, in the settings file) ends while its tab isn't selected, the tab is marked
  done, or failed when the OSC 133 `D` exit code isn't 0, until it is viewed. Needs shell
  integration. Same notification and sound as M5's agent notifications.
- Running time in the tab hover card: "Running for mm:ss" (`h:mm:ss` from an hour on) for the
  command running in a shell tab, from its OSC 133 `C` mark to now, ticking while the card is
  open (`set_card_clock`). This includes an agent typed at a shell prompt (`claude` in a bash
  tab), which gets no hooks and so no "Working · 2m 14s" lap; only Claude Code tabs opened
  from the new-tab menu have that lap.
- Command palette: Ctrl+Shift+P opens a fuzzy search over every action (the ones with
  shortcuts and the menu-only ones such as Duplicate, Move to group or Copy branch), showing
  each one's shortcut, plus the tabs by name ("go to tab"). Enter runs the action on the
  selected tab.
- Themes: light and dark UI (gpui-component's `ThemeMode`, hard-coded to dark in `main.rs`)
  and terminal color schemes: a few bundled schemes plus user schemes imported from
  base16/iTerm files, saved in the settings file. "System" follows the OS light/dark setting.
- Window opacity and background blur, set in the settings file (opacity applies to the
  terminal background only, text stays opaque); blur where the platform allows it (Windows
  acrylic/mica, KDE on X11).
- File viewer find and replace: Ctrl+F searches the open file (highlight all matches,
  Enter / Shift+Enter to move, match count, case and whole-word toggles), Ctrl+H adds a
  replace field (replace one / all) in source mode; rendered Markdown only gets find.
- Configurable font family and size.
- Configurable default shell for the new-tab menu's first entry and Ctrl+Shift+T, saved in the settings
  file (overrides the $SHELL / Git Bash default).
- File explorer "cd ..": a parent-folder button in the explorer header (Lucide `folder-up`
  icon, a folder with an up arrow, added to `crates/app/assets/icons/`; tooltip "Parent folder
  (cd ..)") and/or a `..` row above the root. Both emit `ExplorerEvent::CdTo` with the parent
  folder, so a shell tab types `cd <parent>` and the tree follows the new working directory.
  Disabled at a filesystem or drive root, and in a tab that isn't a shell (`can_cd` false:
  Claude Code, Copilot), since the tree always shows the session's folder.
- Resizable file viewer pane: drag the divider on the viewer's left edge to resize it
  (currently fixed at 45% of the width, `VIEWER_WIDTH` in `app/src/workspace_view.rs`); resize
  cursor on hover, a minimum width for both the viewer and the terminal, double-click resets
  to 45%. The width is kept as a fraction so it follows window resizes, and is saved in the
  settings file and restored at startup.
- File viewer: keep the scroll position when the file is reloaded after a change on disk
  (it goes back to the top); open links to other files (`FEATURES.md`) in the viewer instead
  of the system handler; the `⋮` menu from the reference screenshot.
- New-tab folder picker (currently the selected tab's folder).
- File explorer: read folders off the UI thread (a "Loading..." row, results applied per
  root) so a folder with tens of thousands of entries or a slow network mount doesn't stall
  the window; Delete already runs in the background.
- Sidebar search: Up/Down to move through the matches from the search field; the filter/sort
  button from the reference screenshot.
- Configuration panel: a settings dialog opened from the title bar's settings button (see the
  title bar row above) that edits the settings file, for the configurable items in this list.
- Configurable terminal path: the executable path for each launcher entry (Terminal, Git Bash,
  `claude`, `copilot`) saved in the settings file, for programs that aren't on `PATH` or
  where the wrong copy is found first; an empty value keeps the current lookup.
- Configurable keyboard shortcuts: load a user keymap (keystroke -> action name, built with
  `cx.build_action`) in place of `default_bindings`.
- Configurable terminal icon: the icon for each launcher entry, picked from the bundled
  `LauncherIcon` set (Bash, PowerShell, Command Prompt, Zsh, Fish, Ubuntu, ...) and saved in
  the settings file; it shows in the new-tab menu and on the sidebar row of every tab started
  from that entry (restored tabs included). "Automatic" keeps the current detection
  (`shell_icon` / `distro_icon` in `workspace-model/src/launcher_icon.rs`), e.g. for a
  wrapper script that `shell_icon` only sees as `Shell`.
- Configurable tab background color: a color picked per tab from the tab menu (a small
  palette plus "Default") that tints the tab's sidebar row and its terminal background; saved
  with the tab in `tabs.json` and restored. The terminal's foreground colors stay unchanged,
  so the palette must keep text readable in light and dark themes.
- Drag group headers to reorder groups (the group menu's Move up / down does it for now).
- Draw box-drawing and block characters procedurally; font glyphs leave small gaps between rows for heavy/double verticals (`┃`, `║`).
- Answer OSC 52 clipboard requests.
- Report drags that leave the terminal area (GPUI only delivers moves while hovered).
- Working directory from OSC 7 (only useful for remote shells): `alacritty_terminal` does not
  parse it, but the pty reader wrapper (`term-core/src/prompt.rs`) could find it.
- Untested on Windows: hook command quoting (`"C:\...\agentws-hook.exe"` run by Claude Code's
  shell) and `.cmd` launchers (`claude.cmd` from npm) under ConPTY.

## Risks

- **GPUI API churn:** exact `gpui-pre` version; UI isolated in `app`.
- **IME and clipboard edge cases** in a custom terminal element: test early in M1, especially on Windows.
- **Claude Code `--settings` flag and hook payload shapes:** verify against current docs before relying on them.
