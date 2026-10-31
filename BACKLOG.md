# Agent Workspace - Backlog

Backlog and next steps. The target is [FEATURES.md](FEATURES.md); how built features behave at
their edges is in [docs/known-limitations.md](docs/known-limitations.md); finished work is in
the git history and [CHANGELOG.md](CHANGELOG.md). Remove an item once it is built and verified;
an item only missing its manual check moves to "Fixed, not tested yet", which lists the checks
still to do (Windows included), not what was already tested.

## Fixed, not tested yet

Built with automated coverage on Linux; the manual checks below are still to do.

Down enters the tab list during a search:

- Linux and Windows: type a search, press Down: the ring shows on the match and F2 renames
  it; Up in the field still steps through the matches.

Wine test runner:

- Linux with Wine: run `scripts/test-windows.sh`; the app tests finish serially with only
  `duplicate_shortcut_does_not_send_eof_to_the_original_terminal` skipped. Other crates run
  normally. On real Windows, that test still runs without the Wine workaround.

Copilot Autopilot permission hooks:

- Capture one real Autopilot `PermissionRequest` payload and the following tool hooks. Check
  that Autopilot stays Working without a Waiting triangle, toast or attention shortcut target,
  while a real question still waits. Check mode changes with the optional extension, and
  `--mode autopilot` / `--autopilot` launches without it; `--yolo` alone is not Autopilot.

Sidebar and explorer opacity:

- On a composited Windows/Linux desktop, check Dark, Light and Sepia at 100 % and below:
  normal, selected and hovered sidebar/explorer rows stay readable; the file viewer,
  Settings, title bar, menus, cards, dialogs and toasts stay opaque. Check long explorer lists.
- Recheck the reported opacity leak on the affected desktop with its platform/compositor,
  theme, opacity, blur and a screenshot; distinguish the newly translucent panels from a leak.

Windows Mica client area:

- Build on Windows with the MSVC tools, then on Windows 11 build 26100 check Mica below
  100 % at startup and when selected live: the client area must show the material. Switch
  Mica -> None / Acrylic / 100 % and back, including a restart; the previous material must
  disappear. Check the persistent warning if a DWM operation fails.
- Check Windows 10's Acrylic fallback. Add a restart-only Mica caption/known limitation only
  if the live Windows check actually shows it cannot be removed.

Softer light theme:

- Windows (where the glare was reported): the light UI is comfortable for a long session;
  sidebar, Settings, menus, hover cards, file viewer and toasts look warm grey, not white,
  with readable muted and dimmed text and visible borders. If it is still too bright, decide
  whether to offer light variants (docs/ux-decisions.md, "Light theme too white").
- Status colors on the light sidebar (Working, Needs attention, Ready for review, Idle,
  Running, Recent output), on a normal, hovered and selected row and in hover cards and
  collapsed-group chips; dark theme unchanged.
- Sepia: the same checks as Light (surfaces, muted and dimmed text, status colors on normal,
  hovered and selected rows), plus buttons, switches and sliders in brown.
- Theme dropdown: Dark / Light / Sepia apply live (also Light ↔ Sepia), survive a restart,
  and a hand-edited `ui_theme` still applies.
- Terminal colors (Linux and Windows): Match theme follows each Theme; a fixed scheme
  repaints open tabs, alternate-screen programs (vim, htop) included; `ls --color`, git, dim
  text, selection and search highlights read well on Light and Sepia. Claude Code and Copilot
  CLI started in a light terminal pick readable colors (OSC 11); Windows conpty programs.

Command history per shell:

- Windows: PowerShell, Windows PowerShell, Git Bash and two WSL distros (bash) each record
  under their own chip with the menu's name and icon; a PowerShell tab doesn't suggest a Git
  Bash command and the other way round; a restored tab and the Ctrl+Shift+T default shell
  record under the same chip as the menu's entry; Alt+P / Alt+G / Alt+U switch, AltGr
  (Ctrl+Alt) letters still type in the shell; a removed WSL distro with commands keeps
  its chip, with a generic icon.
- A German or French layout (Linux and Windows): Alt+letter switches; on a Cyrillic layout,
  clicking a chip does.
- Upgrade: with a `history.jsonl` written by the previous version, its commands show under
  "All" only and the previous version still reads the new file.

Dimmed git-ignored entries in the file explorer:

- A machine without git (Windows): nothing dims, with no console window flashing.

Agent status off shows in the UI:

- Windows: without `agentws-hook.exe` next to the app, the warning names
  `agentws-hook.exe`; a quiet Claude Code tab reads "Status unavailable".
- The status listener failing to start (e.g. a socket/pipe that can't be created) shows "could
  not start the status listener (...). Restart the app to retry."; with a broken default shell
  too, both warnings show, agent status first.

Clipboard writes setting:

- Linux: Settings → Terminal → Clipboard Ask / Allow / Deny (Windows: see the Deny bug in
  the Backlog).

Copilot working spinner:

- Linux: minimize/restore through a real window manager.

Feature switches (0.2.20-0.2.24):

- Running badge pulse with Activity animations on and off, on a hardware GPU.
- Terminal history switches: confirming the Clear history / Clear saved session commands
  dialogs, the last-session suggestion across a real restart, zsh and PowerShell, and two
  instances running together.
- File explorer and viewer switch: the "Open externally?" dialog and system launch, "Open
  settings file" while off, a hand edit of `settings.json` to off with an unsaved file, the
  prompt chain with two or more unsaved files, and Windows.
- Busy-close warnings: the Linux title bar's close button (not drivable under Xvfb) and
  macOS window close.
- Clipboard notice: Copilot CLI's copy on Windows (Allow should then copy as before).
- `"keybindings"` overlay: vim's visual block with Ctrl+V unbound, a keystroke sequence
  (`ctrl-k ctrl-c`), the reload on saving `settings.json` in the file viewer, and Windows
  (PowerShell, cmd, Git Bash).
- Keyboard settings page: recording while a dialog or toast has the keyboard, closing the
  Settings row with its × while recording, AltGr and non-US layouts (`setxkbmap de`), the
  Ctrl+number hint in the tab hover card, the clipboard notice tooltips, and Windows/macOS
  (Super/Win and Cmd keys).
- Window opacity and blur: the desktop actually showing through and the blur, on GNOME
  Wayland (see-through, dropdown "Not available on this system", the title bar shadow over
  the background), KDE Plasma Wayland
  (Blur on and off live), X11 with a compositor (see-through) and without one (darker
  background) and Windows 10 (Acrylic; Mica shows the fallback caption and looks like
  Acrylic).

Earlier features (0.2.17-0.2.19), Windows and Linux unless noted:

- Workspace shortcut capture: Ctrl+Shift+D on a live shell, idle and while a command runs:
  the original stays open (no Ctrl+D sent to it); tab navigation and sidebar/explorer
  shortcuts work, and plain Ctrl+D still reaches the shell.
- Last-command persistence: run distinct useful commands in several integrated shell tabs,
  delete a saved command from HISTORY, close and reopen: the latest tab layout and
  suggestions, no deleted suggestion, no saves from a second guest instance. The UI stays
  responsive during frequent commands and shutdown with pending writes.
- Copilot plugin lifecycle: with two Copilot tabs, close/exit/relaunch one: only its plugin
  is removed; after an app crash, startup removes stale plugins but keeps a live instance's
  plugins and unrelated files. Launch agents with a quoted Windows path.
- Initial shell fallback (Windows): the persistent warning keeps the requested shell's
  failure reason.
- Light theme switch: editing `ui_theme` through the file viewer, restart persistence, an
  unchanged terminal palette, and Linux.
- Restore a shell tab's last command: run a useful command then `cd`/`ls`, restart: the
  useful command is suggested; Right/End accept, Tab/Enter pass through, dismissal, the
  narrow-window hint, crash recovery and forgetting across tabs. Windows: Git Bash and bash
  in WSL.
- Agent launch arguments: quoted arguments, empty fields, malformed quotes, Claude
  `--permission-mode plan`/`--model opus` and Copilot `--mode plan`/`--yolo`; after changing
  the settings, duplicated and restored tabs keep the old arguments. On Windows, edits
  survive leaving the field, Enter, closing/reopening Settings and restart; clearing and
  resetting work, new tabs use the saved arguments, and Font family keeps its edits too.

Packaged apps on Windows and Linux: a smoke test and the About dialog's version and commit.

## Backlog

Optional behavior gets one switch per major feature, not per action (FEATURES.md, "Optional
behavior"); items below name their switch where they need one.

- `claude` / `copilot` typed at a shell prompt get agent status in place: today they run in
  the shell tab without hooks and show only "Running for", unlike a tab from the `+` menu.
  Instead, the agent keeps running in the same shell tab, with the same hooks, status
  (Working / Waiting / Ready for review), attention target, notifications and mode reporting
  as a `+` menu agent tab; when it exits, the tab goes back to being a plain shell tab. Steps:
  1. Environment: a shell tab with shell integration gets what `Hooks::attach`
     (`app/src/hooks.rs`) gives an agent tab, without the launch flag: `AGENTWS_SESSION`,
     `AGENTWS_PIPE`, `AGENTWS_HOOK`, plus the Claude settings file path and the tab's
     Copilot plugin folder (created with the tab, removed with it like an agent tab's) in new
     variables. Hook events from that session id must reach the shell tab (today only agent
     tabs use them).
  2. Shell functions (bash, zsh, PowerShell): `claude` and `copilot` print a tokened mark,
     e.g. `ESC ] 133;N;aw=<token>;agent=claude BEL`, then run the real program (`command
     claude`) with `--settings <path>` / `--plugin-dir <path>` put before the user's
     arguments, and return its exit code. `Osc133Scanner` accepts the mark, `PromptTracker`
     reports it as a new `PromptEvent`, and the tab switches to that agent's status policy
     (full hooks for Claude, progress plus attention hooks for Copilot) until the command's
     `D` mark; then it is a shell tab again (status, last-command tracking, busy-close as
     today).
  3. Sidebar while it runs: the agent's icon and status on the row and hover card; the tab's
     saved kind stays shell, so restore and duplicate start a shell (the last-command
     suggestion can offer `claude --resume`). Unit-test the switch in and out of agent status
     (`workspace-model`), including an agent killed with Ctrl+C and a shell exiting mid-run.
  - Run unwrapped (no hooks, as today): the non-interactive forms (`-p` / `--print`,
    `--version`, `--help`, subcommands such as `claude mcp ...` / `claude update`), stdin or
    stdout not a terminal (`echo x | claude -p`), `command claude` / `\claude`, an agent not on
    `PATH` (the shell prints its own "command not found"), nested shells and cmd tabs (no
    functions).
  - Switch "Agent status for agents typed at the prompt" (Terminal page, on): when off, the
    functions aren't defined (the app passes a flag with the token) and typed `claude` /
    `copilot` run as today. Applies to newly started shells.
- Hiding the sidebar (Ctrl+Shift+B) while the search field or a tab name editor has the
  keyboard leaves focus on an element no longer drawn (only the tab list itself hands the
  keyboard back, `toggle_sidebar` checks `is_focused`): check the whole sidebar
  (`contains_focused` plus the search input's focus handle) and focus the terminal.
- Interrupting Claude Code (Esc) leaves the tab Working until the `idle_prompt` notification
  (~60 s); no hook fires on interrupt.
- Long durations on the sidebar and hover card ("Running for", lap times such as "Working · 2m 14s"):
  - Under a day: as today (seconds kept).
  - A day or more: drop the seconds, add days ("1d 3h 12m").
  - A month or more (30 days): add months, drop the hours and minutes ("2mo 5d").
    One shared formatter in `workspace-model`, unit-tested at the boundaries (59 s, 1 d, 29 d 23 h,
    30 d), used by the row, the hover card and the group hover card.
- Long command finished in a shell tab: when a command that ran longer than a threshold
  (default 10 s, in the settings file) ends while its tab isn't selected, the tab is marked
  done, or failed when the OSC 133 `D` exit code isn't 0, until it is viewed. Needs shell
  integration. Same notification and sound as the agent notifications.
  Switch "Long-command completion status" (Notifications page, on): off means no Done/failed
  marks or notifications; command tracking stays for busy-close protection and the threshold
  setting is kept.
- Command palette: Ctrl+Shift+P opens a fuzzy search over every action (the ones with
  shortcuts and the menu-only ones such as Duplicate, Move to group or Copy branch), showing
  each one's shortcut, plus the tabs by name ("go to tab"). Enter runs the action on the
  selected tab.
- File viewer search and replace: Ctrl+F opens a search bar for the open file (highlight all
  matches, Enter / Shift+Enter to move, match count, case and whole-word toggles); Ctrl+H
  adds a replace field with Replace and Replace all in editable source mode. Replacements
  update the editor buffer and mark it unsaved, using the existing Save and unsaved-edit
  confirmation flow; read-only and rendered views only get search. Shortcuts apply while
  the viewer is focused, leaving terminal shortcuts unchanged; Esc closes the bar and
  returns focus to the file.
- File viewer close shortcut: Ctrl+Shift+X closes the selected tab's file viewer from
  either the terminal or the viewer, using the same unsaved-edit confirmation as its close
  button. Return focus to the terminal after closing; do nothing when no viewer is open.
- File viewer: the rendered JSON view ("Not valid JSON") is only worked out when the file
  opens, so it stays stale after the file is fixed and saved in the viewer.
- Command history with zsh-autosuggestions (likely bug, unverified): Enter on a HISTORY row
  sends Ctrl+E, which the plugin binds to accept the grey suggestion, but the reported line
  (`$BUFFER`, `.zshenv` `__agentws_report_line`) excludes it, so the Backspaces leave the
  accepted rest in front of the new command. Send a key that doesn't accept it, or clear
  `POSTDISPLAY` first.
- Drag group headers to reorder groups (the group menu's Move up / down does it for now).
- Zoom with Ctrl+scroll, like Windows Terminal: Ctrl+wheel up / down over the terminal makes
  the font bigger / smaller one point per notch (clamped, e.g. 6-48 pt), also Ctrl+= / Ctrl+-
  and Ctrl+0 to reset. Build on the existing terminal font-size setting, re-measuring cells
  and resizing the pty when zoom changes. The zoom is per tab: it changes
  only the tab under the mouse (or the focused one, for the keys), and a new tab starts at the
  settings size, which acts as the default zoom. Zooming never changes the setting and isn't
  saved (Ctrl+0 and a restart go back to the settings size). Ctrl+wheel is never sent to the
  program, even with mouse reporting on.
- New-tab folder picker (currently the selected tab's folder).
- File explorer: read folders off the UI thread (a "Loading..." row, results applied per
  root) so a folder with tens of thousands of entries or a slow network mount doesn't stall
  the window; Delete already runs in the background.
- File explorer drag and drop: drag a file or folder onto another folder (or a root) to move
  it there, with the drop target highlighted while hovered; a name clash asks before
  replacing, and a folder can't be dropped into itself or its children. Dropping onto the
  terminal types the quoted path (`file_tree::quote_path`) instead. Files dropped from the
  system file manager onto a folder are copied there. The tree and an open viewer follow the
  moved file; moves run in the background like Delete.
- Sidebar search: the filter/sort button from the
  [reference screenshot](docs/ui-reference/sidebar-hover-card.png).
- Configurable tab background color: a color picked per tab from the tab menu (a small
  palette plus "Default") that tints the tab's sidebar row and its terminal background; saved
  with the tab in `tabs.json` and restored. The terminal's foreground colors stay unchanged,
  so the palette must keep text readable in light and dark themes.
- Group hover card (today: the tab count and the Working / Waiting / Ready for review counts):
  - Running shell commands: "Working" counts only agent tabs (Claude Code / Copilot
    `TabStatus::Working`), so a group whose shells run a long command shows "Working: 0".
    `GroupSummary::running` is already counted (the collapsed header shows its chip) but
    `sidebar::group_card` never prints it. Add a "Running: N" line (or rename the lines so
    agent vs. shell is clear) and unit-test the summary for a mixed group.
  - One line per tab with kind icon, name, status and lap time ("Working · 2m 14s",
    "Running for 40s") and branch, in sidebar order; tabs that need attention (Waiting, Ready
    for review or failed) first.
  - Common folder or branch shown once in the header when every tab shares it.
  - Clicking a line selects that tab (expanding the group). The card ticks the lap times while
    it shows (`set_card_clock` for each working tab) and is built only while it shows; no
    card during a drag. Empty groups keep their basic zero-count card.
- Draw box-drawing and block characters procedurally; font glyphs leave small gaps between rows for heavy/double verticals (`┃`, `║`).
- Answer OSC 52 clipboard reads (`ESC ] 52;c;? BEL`): refused today by alacritty's default
  `Osc52::OnlyCopy`; writes are already stored.
- Report drags that leave the terminal area (GPUI only delivers moves while hovered).
- Working directory from OSC 7 (only useful for remote shells): `alacritty_terminal` does not
  parse it, but the pty reader wrapper (`term-core/src/prompt.rs`) could find it.
- Cache the active TerminalView (`.cached(...)`) so sidebar animation frames and other
  WorkspaceView re-renders don't re-render the terminal; every input the view shows (focus,
  resize, selection, session wakeup) must notify it; check with Xvfb.
- Dependency updates (checked 2026-10-20, toolchain 1.99.1):
  - Still behind (major versions): `gpui-kit`, `windows-registry`, `parking_lot` 0.13,
    `generic-array`.
  - Bigger updates, each on its own: `gpui-kit` 0.7.3 -> 0.8.0 (likely API changes in
    GPUI / `gpui-component`), `windows-registry` 0.6 -> 0.100 (its Rust 1.95 minimum is
    already the MSRV).
  - Pre-releases to watch, not adopt yet: `libc` 1.0 (alpha), `notify` 9.0 (rc).
- PowerShell: the integration writes the `C` mark on Enter even when PSReadLine only adds a
  line to an unfinished command (`if ($x) {`), so the tab shows Running while typing; write it
  only for accepted lines (needs a machine with pwsh to check).
- PowerShell: after an app restart, a restored PowerShell tab doesn't show its last command
  (the restored-tab suggestion that bash, zsh and Git Bash tabs get). Find whether the
  command is never saved for PowerShell tabs or saved but not offered on restore.
- Configurable terminal icon: the icon for each launcher entry, picked from the bundled
  `LauncherIcon` set (Bash, PowerShell, Command Prompt, Zsh, Fish, Ubuntu, ...) and saved in
  the settings file (settings page row); it shows in the new-tab menu and on the sidebar row
  of every tab started from that entry (restored tabs included). "Automatic" keeps the
  current detection (`shell_icon` / `distro_icon` in `workspace-model/src/launcher_icon.rs`),
  e.g. for a wrapper script that `shell_icon` only sees as `Shell`.
- Prompt at the bottom ([reference](docs/ui-reference/revers_command_line_order.png)): in a
  shell tab the prompt stays on the last rows, the HISTORY panel (Up) opens just above it, and
  the earlier commands and their output sit above the panel. Today a new tab or `clear` puts
  the prompt at the top and the popup then opens below it. Shell tabs with shell integration
  only; the alternate screen (vim, less, an agent's TUI) and agent tabs draw as today. Steps:
  1. Bottom-anchored grid: while the rows below the cursor are empty, `terminal_element`
     draws the screen shifted down by that many rows (blank space at the top), so the prompt
     line is the last row. Selection, links, mouse reports, the IME box and the history popup
     use the same offset (one `row_offset` in the geometry). No pty resize, so output and
     `tput lines` are unchanged; scrollback and `display_offset` work as before. Unit-test the
     offset (empty screen, full screen, cursor mid-screen after `tput cup`, alternate screen).
  2. Docked HISTORY panel: always above the prompt (drop the below-the-prompt placement and
     `MIN_ROWS_ABOVE`), full terminal width, newest row next to the prompt as now. It pushes
     the rows above the prompt line up by its height instead of covering them, so the last
     command's output stays readable right above it; closing it puts them back.
  3. Command blocks: from the OSC 133 marks, record each finished command's prompt line (as
     an absolute grid line, kept through scrolling and dropped when it leaves the scrollback),
     folder and duration (`C` to `D`). Draw a thin separator above each prompt line and the
     folder and duration ("~/.config/stock-watcher (0.02s)") right-aligned on it, dimmed; a
     failed exit code tints the separator. A resize that reflows lines drops the blocks.
     Switches (Terminal page, both on when built, independent): "Prompt at the bottom" (off:
     today's top-anchored layout; with the history popup off, no docked panel is built) and
     "Command separators and details" (off: no separators, folder or duration labels, and no
     command-block metadata recorded).
     Not planned from the screenshot: the separate input box with the folder chip (the prompt is
     the shell's own, in the grid) and the panel's "Configure" button.
- More agents: OpenCode, OpenAI Codex CLI and Gemini CLI (plus Antigravity's CLI if it ships
  as its own program), each integrated like Claude Code / Copilot CLI today. One agent per
  step, each tested; add them to FEATURES.md when picked.
  - Launcher: found on `PATH` (`launcher.rs`) with a stable id (`opencode`, `codex`,
    `gemini`), an icon in `LauncherIcon`, a `+` menu entry, a launch arguments setting, and
    restored from `tabs.json`.
  - Agent status: find each CLI's hook or event mechanism and map it to Working / Waiting /
    Ready for review through `agentws-hook` and the `HookServer`, set per tab (like Copilot's
    `--plugin-dir`), never written into the user's own config. To check against the installed
    versions: OpenCode plugins (session idle, permission events), Codex's `notify` program
    (turn complete only; look for newer hooks), Gemini CLI hooks. Where events are missing,
    fall back to OSC 9;4 progress or the title, like Copilot, and list the gaps in
    docs/known-limitations.md.
- Idle redraw after tab search (likely bug): once the "Search tabs..." field has had the
  keyboard, the idle window keeps redrawing (about 22 CPU ticks/s in Xvfb, even after going
  back to the terminal; a fresh start idles at 0-2). Find what keeps notifying (cursor blink of
  the unfocused input?) and stop it.

### Git

- Safety rule: avoid actions that can cause conflicts. Block working-tree-changing actions
  (branch switching, pull, merge/rebase) while uncommitted changes exist, including staged,
  unstaged and untracked files. Explain the block; never auto-stash or discard changes.
- Git workflow analysis first: review the existing Git UI and common needs (status and diff,
  local and remote branches, fetch, pull, push, staging and commits, stash, merge/rebase,
  worktrees and remotes). Define the scope, entry points and safe defaults before
  implementation, including authentication, dirty working trees, conflicts, remote tracking
  and repositories shared by running tabs or agents.
- Switch branch from the app: select a local branch or check out a remote-tracking branch.
  Depends on the workflow analysis above and requires a clean working tree; define safeguards
  for active agents and refresh branch/status information for all affected tabs after a switch.

## Parked ideas

Not committed to; not in [FEATURES.md](FEATURES.md) until picked.

- Named workspaces (the single window's tabs are already restored): save and open named
  workspaces, offer to relaunch their tabs; restored tabs clearly marked "not running".
  - Entry point: "Save as new config" in the tab context menu
    ([reference](docs/ui-reference/tab-context-menu.png); "Share session" there is not planned).
  - Opening one replaces every current tab, after a confirmation. No confirmation when the
    window has a single tab and it isn't busy (no command running, agent not Working or
    Waiting): that tab is replaced right away.
  - Each restored tab shows a "Not running" panel with a Start button (the open dialog also
    has "Start all"). Agent tabs start a new session (no `--continue`); shells start as new.
  - Saved: what `tabs.json` holds (kind, folder, names, groups, order, launch arguments),
    plus each tab's file open in the viewer and the viewer and explorer widths. Not the
    scrollback.
- Diff panel: the selected tab's working-tree changes (`git diff` against `HEAD`, untracked
  files included) in the file viewer pane, one collapsible section per file with added and
  removed lines colored, opened from the hover card's `+N` or a title bar button, and
  refreshed when the file watcher sees a change. Shows what an agent changed without leaving
  the app; read-only (no staging or reverting).
- Git worktree and pull request on the sidebar row: a worktree marker when the tab's folder is
  a linked worktree (`.git` is a file), with the main repository in the hover card; the open
  PR's number and state for the branch, from `gh pr view --json` run off the UI thread and
  cached per branch, skipped when `gh` is missing or not logged in.
- Sticky command header: while scrolling back through a shell tab's output, the command that
  produced the top visible line stays pinned above the terminal (from the OSC 133 marks);
  clicking it scrolls to that command's prompt. Needs shell integration; not for agent tabs.
- Secret redaction: text matching secret patterns (API keys such as `sk-...`, `ghp_...`,
  AWS keys, `password=...`, plus user regexes from the settings file) is drawn masked in the
  terminal and copied masked unless the copy is confirmed; hover or a toggle reveals it. Agents
  often print `.env` contents. Display only, the program's output is unchanged.
- Claude Code subagents on the tab: a subagent runs inside the parent `claude` process and has
  no terminal of its own, so it is shown from hook events and its transcript, not as a live tab.
  Claude Code only (Copilot has no subagent hooks).
  - Events: register `SubagentStart` / `SubagentStop` in `HOOK_EVENTS`, take the description
    and `subagent_type` from `PreToolUse` with `tool_name` `Agent` (formerly `Task`), and tag
    tool events by `agent_id`. `hook_line` (`agent-status/src/ipc.rs`) forwards only the event
    name and `notification_type` today, so the wire format must carry these fields. Check the
    field names against the installed Claude Code first (log one real payload).
  - Step 1, nested rows under the tab card: one row per subagent with type, description,
    status (running / done / failed) and elapsed time; per-tab list in `workspace-model`,
    unit-tested. Not saved in `tabs.json`.
  - Step 2, transcript view: clicking a row opens a read-only pane (like the file viewer) that
    tails `~/.claude/projects/<project>/<session>/subagents/agent-<id>.jsonl` with a file
    watcher and shows its messages and tool calls. The JSONL format is undocumented: parse
    leniently and fail gracefully.
  - Not planned: real child tabs in the tab layout (pins, order and persistence for
    short-lived, non-relaunchable tabs).
- Notifications and close warnings:
  - Sound for agent events (Waiting, Ready for review): one bundled sound for both events
    (`rodio`). "Notification sounds" switch on the Notifications page, off by default (today
    is silent): off means no agent, long-command or bell sound, and no audio initialization;
    the volume slider stays, disabled, for when it is on. Visual status is unaffected.
    Parked: `rodio` needs the ALSA dev package (`libasound2-dev`) to build on Linux, also in CI.
