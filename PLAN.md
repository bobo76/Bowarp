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
- Copy/paste on GPUI actions: Ctrl+Insert added for copy; all clipboard keys are now key
  bindings (ready for a configurable keymap). Verified under X11 (Xvfb): Ctrl+Insert copy,
  Shift+Insert and Ctrl+Shift+V paste.
- Tests: 74 passing (`term-core` session + palette, `agent-status` state machine + IPC,
  `agentws-hook` round trip with the real binary, `app` key/paste translation, mouse reports,
  wheel decision, render batching/colors, scroll bar geometry, launcher discovery).
- Verified by driving the app under X11 (screenshots + simulated input). Wayland checked
  manually by the user.
- M1 "done when" check passed on Ubuntu (Wayland) on 2026-10-17: 3 Claude Code sessions +
  1 shell; steps in [README.md](README.md#manual-check).
- M2 parts A + B (branch `m2-sidebar`, 2026-10-17): two-line tab rows (title; branch or folder),
  status badge on the session icon, warning triangle on Needs attention, Done status, bundled
  Claude spark and git-branch icons, sidebar toggle (title bar button, Ctrl+Shift+B). Folder
  follows `cd` via `/proc/<pid>/cwd`; branch read from `.git/HEAD` (`workspace-model`).
  Verified under X11 (Xvfb) with fake hook events: Done / Needs attention / Working badges, Done
  clearing on select, `~/.config` folder line, sidebar toggle. 83 tests passing.
- M2 part C (2026-10-17): tab menu from `⋮` and right-click (`tab_menu.rs`): pin/unpin, copy
  branch / pane title / working directory, inline rename, move up/down, close / close others /
  close below, color swatches. Tab model (`TabMeta`: custom name, pinned, color) and ordering
  rules (`workspace_model::order`: pinned block at the top, moves stay in their block, close
  others/below spare pinned tabs). Menu actions address tabs by session id, so they hit the
  right tab after moves. The window title stays the program's title; the custom name is the
  sidebar's. Verified under X11 (Xvfb): every menu entry, disabled states, Escape cancelling a
  rename, clipboard contents. 89 tests passing.

- M2 part D (2026-10-17): groups and drag & drop. `workspace_model::Layout` (replaces `order`)
  holds the pinned block, then ungrouped tabs and groups; pinned tabs are never grouped, groups
  stay when empty, new tabs join the selected tab's group. Tab menu: New group with tab (starts
  an inline rename), Move to group › (other groups, Remove from group). Group header: chevron,
  color dot, name, "N tabs", status chips when collapsed (attention / working / done), `⋮`
  and right-click menu (rename, collapse, move group up/down, close tabs, ungroup / delete,
  color). Tabs without their own color show the group's. Selecting a hidden tab expands its
  group. Drag a tab row: the half of a row picks above/below in that row's block, a header's
  upper half drops above the group and its lower half into it, the space below the list drops
  ungrouped at the end; a marker line shows the spot. Verified under X11 (Xvfb): new group +
  rename, drop into a group / an empty group / the end / the pinned block, collapse with
  chips (fake hook events), Ctrl+3 into a collapsed group, group color inheritance, submenu,
  move group up, ungroup. Not driven: "Close tabs in group". 98 tests passing.
- M2 part E (2026-10-17): search, hover card, jump to next attention. "Search tabs..." field
  in the sidebar header (Ctrl+Shift+S) filters by name, program title, folder (`~` form),
  branch and group name; every whitespace-separated word must match, case-insensitive
  (`workspace_model::TabQuery`). Groups with a match show expanded while searching; Enter
  selects the first match and clears, Escape clears. Hover card (gpui-component `HoverCard`,
  0.6 s delay) right of the row: status chip, Ctrl+N, full folder (wrapped), branch, program
  title, session kind, `+N` from `git --no-optional-locks diff HEAD --numstat` run on the
  background executor when the card opens, cached per tab and folder for 5 s
  (`workspace_model::lines_added`). It replaces the status tooltip on the icon. Ctrl+Shift+A
  and a "⚠ N" button next to the search field go to the next other tab needing attention,
  else the next Done tab (`workspace_model::next_attention`). Verified under X11 (Xvfb) with a
  stand-in `claude` script and fake hook events: card with Idle/Done/No recent output and
  `+25` on a repo with 25 added lines, card for a shell outside a repo (no branch, no `+N`);
  search by title, group name, folder, two words (`MAIN reload`), no match, Escape (focus back
  in the terminal), Enter, clear button, Ctrl+Shift+S with the sidebar hidden; jump cycling
  shell → attention → Done → attention, the button click, jump into a collapsed group
  (expands it). Not driven: drag & drop while a search is active, a real Claude Code session.
  109 tests passing.
- In-progress lap time (2026-10-17): the hover card's status chip shows how long a Claude Code
  session has been working on the current turn (`Working · 2m 14s`; `45s`, `1h 05m`). Tool
  events keep the lap running; any other status (Needs attention, Idle, Exited) ends it and the
  next Working starts from 0 (`workspace_model::Lap`, `format_lap`). While a card is open a task
  repaints on each whole second of the lap, only while working; closing the card drops the
  task. Verified under X11 (Xvfb) with fake hook events: the chip ticks with the card open
  (1s -> 4s), `PostToolUse` keeps it running, `Stop` shows plain Idle, a new prompt restarts
  at 0s. 123 tests passing.
- Window bounds restore (2026-10-17): the window reopens with the size, position and
  maximized/fullscreen state it closed with, saved as `window.json` in the config folder
  (`directories`: `~/.config/agent-workspace`, `%APPDATA%\agent-workspace\config`). Saved
  just before the window closes, from every close path: last tab closed, title bar `×`, the
  window manager's close (`on_window_should_close`). Ignored (default 1000x650 centered) when
  the file is missing, not valid JSON, smaller than 100 px or less than 48x48 px of it lies
  on any connected display (`workspace_model::WindowState`). A maximized window saves the
  bounds GPUI reports (on X11 the maximized size, so un-maximizing after a restart keeps that
  size). Verified under X11 (Xvfb, no window manager, separate `XDG_CONFIG_HOME`): resize +
  move then close via the last tab and via `WM_DELETE_WINDOW` save the new bounds and the
  relaunch opens there; off-display, partly-visible and corrupt files; an unwritable config
  folder only prints an error. GPUI's X11 backend opens new windows 2 px right of the
  requested x, so a restored window can creep 2 px right per restart under a window manager
  that reports the real position. Not driven: maximize (no window manager under Xvfb: the
  saved mode is not detected and `zoom` has no effect), the title bar `×` (no window controls
  without a window manager), Wayland (position is up to the compositor), Windows. 130 tests
  passing.

**Bugs to fix**

- Windows: the title bar's sidebar toggle button does nothing when clicked; Ctrl+Shift+B
  works. Cause: gpui-component's `TitleBar` marks its whole child area
  `WindowControlArea::Drag`, and GPUI's hit test lists every hitbox under the cursor, so
  `WM_NCHITTEST` answers `HTCAPTION` over our button too. The mouse-down reaches GPUI as a
  non-client event, `Button` does not stop it, so `DefWindowProc` starts the window-move loop,
  which eats the mouse-up: no click. Fix: wrap the title bar buttons in a `div().occlude()`
  (hitbox `BlockMouse` stops the hit test before the drag area, so they get `HTCLIENT` and
  normal clicks); the explorer button goes in the same wrapper.

**Next steps, in priority order**

1. Reopen with the same tabs: kind, program, folder, custom name, color, pin, groups and order
   (`Layout` + `TabMeta` to serde), and the selected tab. Each tab is relaunched in its last
   folder. This brings forward the save/restore part of M4 for the single window; named
   workspaces stay in M4.
2. `+` menu look ([reference](docs/ui-reference/new-tab-menu.png)): an icon per entry (PowerShell,
   Windows PowerShell, Command Prompt, Git Bash, each WSL distro, Claude, Copilot) instead of
   the shared terminal icon, a "Terminal" entry with its `Ctrl+Shift+T` hint, and separators
   between shells and agents.
3. Windows: "Bash" opened Ubuntu. `bash` on `PATH` resolves to `C:\Windows\System32\bash.exe`,
   the WSL launcher, which starts the default distro; Git Bash is not on `PATH` (only
   `Git\cmd` is). Code done in `launcher.rs`, not yet built or run on Windows: `bash` is no
   longer looked up on `PATH`; "Git Bash" is `Git\bin\bash.exe -i -l` found from `git.exe` on
   `PATH`, else `%ProgramFiles%\Git` / `%LOCALAPPDATA%\Programs\Git`; each WSL distro (from
   `HKCU\...\Lxss`, so no `wsl.exe` runs at startup; `docker-desktop*` hidden) is its own entry
   launching `wsl.exe -d <name>`, and its tab is named after the distro.
4. Ctrl+Shift+T opens a new tab with the default launcher (in the selected tab's folder)
   instead of duplicating the selected tab. The default is Bash for now: Git Bash on Windows,
   `bash` on Linux; falls back to the first shell found. A configurable default comes with
   settings. The `+` button's hover tooltip shows the shortcut ("New tab (Ctrl+Shift+T)"),
   and the default entry in the `+` menu shows `Ctrl+Shift+T` on its right. Duplicating the
   selected tab moves to Ctrl+Shift+D, also in the tab menu as "Duplicate tab" with its
   shortcut shown.
5. Taller tab rows with separators ([reference](docs/ui-reference/tab-row-spacing.png)): more vertical
   padding per row (about 56 px instead of the current tight two lines), a larger session
   icon, and a thin full-width divider line (`sidebar_border` color) between rows instead of
   the 2 px gap. The selected/hovered background still fills the whole row; group headers and
   indented group children keep the same rhythm.
6. Collapsible file explorer ([reference](docs/ui-reference/file-explorer.png)): a panel
   between the sidebar and the terminal, shown/hidden by a title bar button next to the
   sidebar toggle and by Ctrl+Shift+| (bound as the key that produces `|`, so check it on
   non-US layouts), closed with its `×`. Root is the selected tab's folder and
   follows tab switches and `cd`. Tree with chevrons, folders first then files, sorted by
   name, dotfiles shown; folder and file-type icons (Markdown, image, ...). Folders are read
   only when expanded (no recursive scan); expanded state is kept per root. Header row with
   the files view icon; the reference's comments and search views are out of scope for now.
   Open questions: what clicking a file does (insert its path in the terminal, open in the
   default app), and refreshing on file changes (`notify` watcher on expanded folders only).
7. Command history in shell tabs (both need shell integration, below):
   - **Inline autocomplete** ([reference](docs/ui-reference/history-autocomplete.png)): while
     typing at the prompt, the most recent history entry starting with the typed text is drawn
     dimmed after the cursor (ghost text, not sent to the shell); Right arrow / End at the end
     of the line accepts it by sending the remaining characters.
   - **History popup** ([reference](docs/ui-reference/history-popup.png)): "HISTORY" panel
     above the prompt listing past commands that contain the prompt's current text, most
     recent at the bottom next to the prompt, with the folder chip; Up/Down to move, Enter to
     put the command on the prompt (replacing the typed text), Esc to dismiss.
   - **History store:** commands executed in any shell tab of the app, with folder and time,
     de-duplicated (latest wins), capped (e.g. 10 000), persisted in the config folder. Not
     the shell's own history file.
   - **Shell integration:** the app must know where the prompt's command line starts and when
     a command runs: OSC 133 (A prompt start, B command start, C executed, D finished) emitted
     by small scripts injected at launch for bash / zsh / pwsh (`--rcfile` / `ZDOTDIR` /
     `-NoExit -Command`), sourcing the user's own config first. `alacritty_terminal` does not
     parse OSC 133, so this needs the pty reader wrapper also planned for OSC 7. The command
     text is read from the grid between the B mark and the cursor. Disabled on the alternate
     screen and in Claude Code / Copilot tabs; cmd.exe and WSL shells without the script get
     neither feature.
   - **Opening the popup:** Up arrow at the prompt always opens it and is no longer sent to
     the shell (the shell's own Up history is dropped where integration is active): all
     history on an empty prompt, filtered by the typed text otherwise. The list has a max
     height (about 10 rows) and scrolls, starting at the bottom with the most recent command
     selected. Up still goes to programs (alternate screen, Claude Code, `less`, ...) and to
     shells without integration.
8. Finish M2: icon overrides per tab (tab menu), the sidebar filter/sort button.
9. M3: split panes, adjustable sizes, pane shortcuts.

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
│   ├── workspace-model/  # tab location (folder, git HEAD); groups/tabs/colors/layout + JSON persistence (M2+)
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
   - Copy/paste: Ctrl+Shift+C / Ctrl+Insert copy, Ctrl+Shift+V / Shift+Insert paste; Linux
     primary selection on select + middle click. The keys are GPUI key bindings to `terminal::Copy`
     / `terminal::Paste` actions in the `Terminal` context (`default_bindings` in
     `terminal_view.rs`), so a user keymap can replace them later.
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

## Target UI

Reference screenshots: [sidebar + hover card](docs/ui-reference/sidebar-hover-card.png),
[tab context menu](docs/ui-reference/tab-context-menu.png). Elements and where they land:

| Element | Look | Milestone |
|---|---|---|
| Title bar | sidebar toggle (collapse/expand the sidebar) - done; settings button once there are settings | M2 |
| Sidebar header | "Search tabs..." field (filters by title, folder, branch) - done; next-attention button - done; filter/sort button - not done; `+` | M2 |
| Tab row, line 1 | session icon in a rounded square, title cut with a trailing ellipsis (as in the screenshot) - done | M2 |
| Tab row, line 2 | git branch (`⑂ main`) when the folder is a repo, else the folder (`~/.config/...`), dimmed - done | M2 |
| Status badge | small round badge on the icon's bottom-right corner (replaces the status dot); warning triangle before the title when it needs attention - done | M2 |
| Selected row | raised background + border, `⋮` menu and `×` close on the selected/hovered row - done | M2 |
| Hover card | status chip (`✓ Done`, `Working · 2m 14s`), full folder, branch, title, session kind with icon, `+N` (git lines added in the working tree, green) - done | M2 |
| Groups | chevron, name, "N tabs" count, indented children; aggregate status when collapsed - done | M2 |
| Drag & drop | drag a tab row to reorder or to drop it into/out of a group; insertion marker while dragging - done | M2 |
| Context menu | see below | M2 / M3 / M4 |
| Clipboard notice | banner above the terminal when a program writes the clipboard (OSC 52), with allow/deny | M5 |

Context menu entries:

- M2: Pin tab, New group with tab, Move to group ›, Copy branch name, Copy pane title, Copy working
  directory, Rename tab, Move Tab Up / Down, Close tab, Close other tabs, Close Tabs Below,
  color swatches (none + 7 colors). All done.
  "Share session" from the screenshot is not planned.
- M4: Save as new config (= save as a named workspace).

New work the look needs:

- **Working directory per tab:** done on Linux: the child's cwd (`/proc/<pid>/cwd`) re-read on
  `TabChanged` and when the tab is shown, never polled; the last known folder is kept after
  exit. Windows (`NtQueryInformationProcess` PEB read) shows the launch folder until then.
  Not done: OSC 7 (`alacritty_terminal` does not parse it; needs a pty reader wrapper, only
  useful for remote shells) and the `cwd` from Claude Code hook payloads (Claude's process cwd
  is its project folder, which `/proc` already gives).
- **Git branch:** done: read `<repo>/.git/HEAD` (walk up from the cwd, follow `gitdir:` for
  worktrees), no `git` process; detached HEAD shows the short commit.
- **Diff stat (`+N`):** done: `git diff HEAD --numstat` plus the lines of untracked files
  (`git ls-files --others --exclude-standard`, files over 1 MiB skipped) run off the UI thread
  only when the hover card opens, one run per tab at a time, killed after 10 s, cached per tab
  for 5 s. On Windows the `git` child may need
  `CREATE_NO_WINDOW` (untested).
- **"Done" status:** done: a Claude tab that goes Working -> Idle while not selected shows Done
  (check badge) until it is selected, then Idle.
- **Tab model:** custom name and color (`TabMeta`); pinned block, groups and order (`Layout`)
  - done. Persistence (serde) with M4.
- **Icons:** done: our own Claude spark and Lucide's git-branch bundled (`app/src/assets.rs`);
  Copilot and shells use the library's GitHub and terminal icons.

## Later Milestones

- **M2:** the [Target UI](#target-ui) sidebar: two-line tab rows, status badges, hover card,
  search, pinning, context menu; named collapsible groups, drag & drop of tabs, colors with tab inheritance, icon
  overrides, aggregate status on collapsed groups, "jump to next session needing attention".
- **M3:** split panes, adjustable sizes, pane shortcuts.
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
- Configurable keyboard shortcuts: load a user keymap (keystroke -> action name, built with
  `cx.build_action`) in place of `default_bindings`.
- Interrupting Claude Code (Esc) leaves the tab Working until the `idle_prompt` notification
  (~60 s); no hook fires on interrupt.
- New-tab folder picker (currently the selected tab's folder).
- Drag group headers to reorder groups (the group menu's Move up / down does it for now).
- Sidebar search: Up/Down to move through the matches from the search field; the filter/sort
  button from the reference screenshot.
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
