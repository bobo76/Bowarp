# Agent Workspace - Backlog

Backlog and next steps. The target is [FEATURES.md](FEATURES.md); how built features behave at
their edges is in [docs/known-limitations.md](docs/known-limitations.md); finished work is in
the git history and [CHANGELOG.md](CHANGELOG.md). Remove an item once it is built and verified;
an item only missing its manual check moves to "Fixed, not tested yet".

## Next steps

Run in this order (each stands alone and is tested):

1. Fix window opacity leaking outside the terminal: below 100 % the file explorer, file
   viewer tabs and other panes turn see-through too (reported by the user, 2026-10-25), but
   only the terminal's default background should (docs/ux-decisions.md, "Opacity: what turns
   translucent below 100 %?"). Find every pane that paints no opaque background of its own.
2. Hide the file viewer while Settings shows (docs/ux-review.md #1): while `shows_page()`,
   don't render the viewer or its divider; the tab keeps its open file and unsaved edits and
   the viewer comes back with the tab. "Open settings file" (opens `settings.json` in the
   viewer) must then close the page, or the file opens out of sight. The explorer stays.
3. No hover card while a rename editor, a context menu or a dialog is open
   (docs/ux-review.md #9), like the existing "no card during a drag" rule.
4. Attention toast says what the agent waits for (docs/ux-review.md #13): keep the hook's
   `notification_type` in `agent-status` and show "Waiting for permission" / "Waiting for
   your reply"; bell and clipboard keep "Needs attention". Add the jump key
   (Ctrl+Shift+A, from the keymap) as a tooltip on the toast.
5. Agent status disabled shows in the UI (docs/ux-review.md #14): when `ClaudeHooks::start`
   fails (`agentws-hook` missing next to the app, or the hook server can't start), show the
   persistent warning used for the shell fallback ("Agent status is off: agentws-hook not
   found next to the app. Reinstall to fix.", with the real reason) and give agent tabs the
   card status "Status unavailable" instead of Idle / No recent output. Shown once per
   start; dismissing hides it for the run.
6. Closing the last tab keeps the window and the workspace (docs/ux-review.md #21, option A,
   picked by the user): the window stays open with an empty state in the terminal area
   ("New tab" buttons for the launchers) and the sidebar still lists the groups, empty ones
   included; colors and groups are saved as usual. Closing the window itself works as today.
7. Dim git-ignored entries in the file explorer (docs/ux-review.md #24): one
   `git check-ignore --stdin` per expanded folder, off the UI thread, refreshed with the
   folder watcher. Folder order unchanged; folders outside a repository show as today.

## Fixed, not tested yet

Implemented with automated coverage (Linux); the manual checks below are pending.

Clipboard writes setting:

- Settings → Terminal → Clipboard: the dropdown shows Ask / Allow / Deny and applies live;
  with Allow a Claude Code tab starts without the clipboard notice, with Deny nothing is
  copied (also in a tab set to "Always for this tab"); Linux and Windows.

Copilot working spinner (driven under Xvfb with a stand-in `copilot` and a forced pulse):

- With the real Copilot CLI on a hardware GPU: the ring turns while a turn runs, stops at
  its end and on a permission prompt (the triangle replaces it), and looks right in the
  light theme. Minimize/restore through a real window manager (Xvfb only unmapped the
  window) on Linux and Windows: no CPU while minimized, turning again after restore.

Feature switches 0.2.20-0.2.24 (Xvfb-driven except as noted):

- Running badge pulse with Activity animations on/off: not drivable under Xvfb (software
  rendering disables the pulse); only the pure decision is unit-tested.
- Terminal history switches: confirming the Clear history / Clear saved session commands
  dialogs, the last-session suggestion across a real restart, zsh and PowerShell, and two
  instances running together.
- Workspace switches: toggling Restore previous tabs / Remember last shell command
  mid-session and accepting the last-command suggestion with Right; not driven.
- File explorer and viewer switch: the "Open externally?" dialog and system launch, "Open
  settings file" while off, a hand edit of `settings.json` to off with an unsaved file, the
  prompt chain with two or more unsaved files, and Windows.
- In-app notifications switch: the "Needs attention" and "Exited" toasts (only Ready for
  review was driven; the rest are covered by a gpui test).
- File panes off, last Discard (fo-files review): `drop_file_panes` calls `focus_active` while
  the Discard dialog is closing; the dialog may restore focus to the dropped viewer.
  Manual check: edit a file, turn the switch off, Discard; the terminal must take keys.
- Busy-close warnings: the Linux title bar's close button (not drivable under Xvfb) and
  Windows/macOS window close.
- Clipboard notice: Copilot CLI's copy on Windows (Allow should then copy as before).
- `"keybindings"` overlay (Xvfb-driven: `ctrl-v: null` gives bash `^V`, a new binding, the
  banners, a hand edit applied by opening the settings page). Not driven: vim's visual block
  with Ctrl+V unbound, a keystroke sequence (`ctrl-k ctrl-c`), the reload on saving
  `settings.json` in the file viewer, and Windows (PowerShell, cmd, Git Bash).
- Keyboard settings page (Xvfb-driven: record with a conflict, +, ×, Reset, Reset all, Esc,
  page change, hints in the new-tab tooltip and tab menu). Not driven: recording while a
  dialog or toast has the keyboard, closing the Settings row with its × while recording, AltGr
  and non-US layouts (`setxkbmap de`), the Ctrl+number hint in the tab hover card, the
  clipboard notice tooltips, and Windows/macOS (Super/Win and Cmd keys).
- Window opacity and blur (Xvfb-driven: the slider saves `window_opacity` and applies live,
  a hand-written value applies at start, the disabled blur dropdown and its captions, only
  the terminal background and padding turn translucent, the file viewer, explorer, menus and
  hover cards stay opaque). Not seen anywhere (Xvfb has no compositor): the desktop actually
  showing through and the blur. Manual checks: GNOME Wayland (see-through, dropdown "Not
  available on this system", the title bar shadow over the background), KDE Plasma Wayland
  (Blur on and off live), X11 with a compositor (see-through) and without one (darker
  background), Windows 10 (Acrylic; Mica shows the fallback caption and looks like Acrylic)
  and Windows 11 22H2+ (Acrylic, Mica), maximized and full screen, and the light theme.
  Windows 11: switch None/Acrylic/Mica live at 80 %, in both directions and through 100 %; if
  Mica stays after leaving it, add the caption "Mica stays until the app restarts" (see
  docs/ux-decisions.md).

Skipped or partially passed in the user's Windows check on 2026-10-23 (0.2.18):

- Workspace shortcut capture (0.2.19): Ctrl+Shift+D duplicates without sending Ctrl+D/EOF
  to the original terminal; other workspace shortcuts are consumed while plain terminal
  shortcuts still pass through.
  Manual Windows/Linux check: duplicate a live shell while it is idle and while a command
  runs; verify the original stays open, tab navigation and sidebar/explorer shortcuts work,
  and plain Ctrl+D still reaches the shell.
- Background last-command persistence (0.2.19): an ordered writer saves snapshots without
  blocking UI storage work, and close drains the final state before releasing ownership.
  A crash can lose queued/in-flight updates, including HISTORY deletion.
  Manual Windows/Linux check: run distinct useful commands in several integrated shell tabs,
  delete a saved command from HISTORY, close and reopen, and verify the latest tab layout and
  suggestions, no deleted suggestion and no saves from a second guest instance. Check UI
  responsiveness during frequent commands and shutdown with pending writes.
- Copilot plugin lifecycle and Windows argument guidance (0.2.19): cleanup covers session
  exit/close/replacement, failed launch and stale dead-owner directories; settings explain
  single-quoted Windows paths and doubled backslashes.
  Manual Windows/Linux check: use two Copilot tabs, close/exit/relaunch one and verify only
  its plugin is removed; after an app crash, verify startup removes stale plugins but keeps
  a live instance's plugins and unrelated files. Launch agents with a quoted Windows path.
- Initial shell fallback: a persistent warning retains the requested shell's failure reason.
  User skipped the Windows check.
- Persisted Light theme switch: Windows passed for readability (Sidebar, Settings, menus,
  viewer, hover cards). Not checked: editing `ui_theme` through the file viewer, restart
  persistence, an unchanged terminal palette, Linux.
- Restore a shell tab's last command (0.2.17): integrated bash/zsh tabs (including Git Bash
  and bash in WSL; not PowerShell, cmd or agents) save the last non-trivial, recordable
  command at command start. `tabs.json` is owner-only on Unix at every write. The first empty
  prompt offers a one-shot dimmed suggestion with a fainter last-session hint when it fits;
  Right or End accepts, never Tab or Enter. Any line action dismisses it, including keys
  before the first prompt; HISTORY deletion clears it from all tabs.
  Manual check: run a useful command then `cd`/`ls`, restart and verify the useful command
  is suggested; check Right/End, Tab/Enter pass-through, dismissal, narrow-window hint,
  crash recovery and forgetting across tabs. User skipped the Windows check.
- Agent launch arguments (0.2.17): Terminal shows Claude/Copilot paths and free-text argument
  fields (`claude_args`, `copilot_args`). Shell-style quoting groups words without shell
  expansion; malformed quoting blocks launch with an error. Arguments follow app hook
  arguments; user `--settings` is passed through even if it overrides hooks. Menu/explorer
  launches use current settings, while existing, duplicated and restored tabs keep original
  args. Ctrl+Shift+T continues opening the default shell.
  Manual check: launch with quoted arguments, empty fields, malformed quotes, Claude
  `--permission-mode plan`/`--model opus` and Copilot `--mode plan`/`--yolo`; change settings
  and confirm duplication/restoration keep the old arguments. User skipped the Windows check.

Not yet checked anywhere: Windows and Ubuntu CI for 0.2.19-0.2.26 (tags were pushed before
CI ran), native Windows/Linux UI checks, packaged-app smoke tests and the About dialog's
version and commit.

## Backlog

Optional behavior gets one switch per major feature, not per action (FEATURES.md, "Optional
behavior"); items below name their switch where they need one.

- Renamed Claude tabs should also get the working spinner: a custom name hides
  Claude Code's animated title glyph, so a renamed Claude tab shows no working animation
  today (the spinner is Copilot only).
- Tab menu "Reset name": shown only when the tab has a manual name (`TabMeta::custom_name`);
  clears it so the tab goes back to the program's title (the agent's AI-generated name for
  Claude Code / Copilot tabs, the shell's title otherwise), saved in `tabs.json` like a rename.
  Renaming to blank already does this today but nobody finds it. Unit-test that the name
  follows the program's title again after the reset.
- Light theme: soften the palette; the user found it too white and hard on the eyes
  (Windows, 2026-10-23). Recheck comfort after.
- `claude` / `copilot` typed at a shell prompt open an agent tab: instead of running in the
  shell tab, the command opens a new tab exactly like the `+` menu's "Claude Code" / "Copilot
  CLI" entry (`open_launcher` in `app/src/workspace_view.rs`, so hooks and status work), in the
  shell's current folder, and selects it; the shell tab stays at its prompt. Steps:
  - The shell integration scripts (bash, zsh, PowerShell) define `claude` and `copilot`
    functions that print a new tokened mark, e.g. `ESC ] 133;N;aw=<token>;agent=claude;
    args=<hex>;cwd=<hex> BEL`, and return 0 without starting the program. `Osc133Scanner`
    accepts it, `PromptTracker` reports it as a new `PromptEvent`, and `TerminalView` asks
    `WorkspaceView` to open the agent's launcher with the folder and the arguments
    (`claude --resume` opens a tab running `claude --resume`).
  - Run in place, not in a new tab: the non-interactive forms (`-p` / `--print`, `--version`,
    `--help`, subcommands such as `claude mcp ...` / `claude update`), a command whose stdin or
    stdout isn't a terminal (`echo x | claude -p`), and `command claude` / `\claude`, which skip
    the function. Agent not found on `PATH` (no launcher program): run in place, so the shell
    prints its own "command not found".
  - Switch "Open agents typed at the prompt in a new tab" (Terminal page, on): when off, the
    functions aren't defined (the app passes a flag with the token) and typed `claude` /
    `copilot` run normally. Applies to newly started shells.
  - A `claude` run in place (`-p` in a script, a nested shell, cmd tabs, which get no functions)
    gets no hooks and shows "Running for" until it quits. Accepted.
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
- Command history per shell: today every shell tab shares one history, so a PowerShell tab
  suggests bash commands and the other way round.
  - Saved by shell: `HistoryEntry` gets a `shell` field, the tab's launcher id (`pwsh`,
    `powershell`, `git-bash`, `zsh`, `wsl:Ubuntu`), so Git Bash and a WSL bash stay apart
    (their paths differ). `#[serde(default)]`: entries saved before have no shell and show
    only under "All". A command is kept once per shell (dedup on shell + command), and the
    cap applies per shell, so a busy shell doesn't push the others out.
  - Inline suggestion: from the tab's own shell only.
  - HISTORY header: one chip per shell that has entries plus "All", using the launcher's
    menu name and icon; the tab's shell is selected when the popup opens. The list shows the
    selected chip's commands, still filtered by the typed text. Delete forgets the command
    in the selected shell only ("All": in every shell).
  - Switching: Tab (pick) and Left/Right (go to the shell, edit the line) are taken, and plain
    letters are typed into the line as the filter, so a bare first letter can't switch.
    Proposed: Alt+the chip's first letter (Alt+P PowerShell, Alt+G Git Bash, Alt+U Ubuntu,
    Alt+A All), with the letter underlined on the chip. Two chips with the same letter
    (PowerShell and Windows PowerShell): pressing it again moves to the next one. Ctrl+Alt is
    left alone (AltGr on European layouts). Clicking a chip also switches. While the popup
    is open, Alt+letter isn't sent to the shell (so readline's Alt+B / Alt+F word moves are
    lost there). Alternatives: Ctrl+Left / Ctrl+Right to move through the chips (no letter
    collisions, but slow with many shells), or Shift+Tab to cycle.
  - Unit-test the JSON round trip (field missing, present), per-shell dedup and cap, the
    suggestion per shell, and the chip key (letter collision, Ctrl+Alt ignored).
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
- Sidebar search: the filter/sort button from the
  [reference screenshot](docs/ui-reference/sidebar-hover-card.png).
- Configurable tab background color: a color picked per tab from the tab menu (a small
  palette plus "Default") that tints the tab's sidebar row and its terminal background; saved
  with the tab in `tabs.json` and restored. The terminal's foreground colors stay unchanged,
  so the palette must keep text readable in light and dark themes.
- Group hover card leaves out running shell commands: its "Working" line counts only agent
  tabs (Claude Code / Copilot `TabStatus::Working`), so a group whose shells run a long command
  shows "Working: 0". `GroupSummary::running` is already counted (the collapsed header shows
  its chip) but `sidebar::group_card` never prints it. Add a "Running: N" line (or rename the
  lines so agent vs. shell is clear) and unit-test the summary for a mixed group.
- Extended group hover card (basic counts are implemented): one line per tab with kind icon,
    name, status and lap time ("Working · 2m 14s", "Running for 40s") and branch, in sidebar
    order; tabs that need attention (Waiting, Ready for review or failed) first.
  - Common folder or branch shown once in the header when every tab shares it.
  - Clicking a line selects that tab (expanding the group). The card ticks the lap times while
    it shows (`set_card_clock` for each working tab) and is built only while it shows; no
    card during a drag. Empty groups keep their basic zero-count card.
- Drag group headers to reorder groups (the group menu's Move up / down does it for now).
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
    GPUI / `gpui-component`), `windows-registry` 0.6 -> 0.100 (needs Rust 1.95).
  - Pre-releases to watch, not adopt yet: `libc` 1.0 (alpha), `notify` 9.0 (rc).
- PowerShell: the integration writes the `C` mark on Enter even when PSReadLine only adds a
  line to an unfinished command (`if ($x) {`), so the tab shows Running while typing; write it
  only for accepted lines (needs a machine with pwsh to check).
- Configurable terminal icon: the icon for each launcher entry, picked from the bundled
  `LauncherIcon` set (Bash, PowerShell, Command Prompt, Zsh, Fish, Ubuntu, ...) and saved in
  the settings file (settings page row); it shows in the new-tab menu and on the sidebar row
  of every tab started from that entry (restored tabs included). "Automatic" keeps the
  current detection (`shell_icon` / `distro_icon` in `workspace-model/src/launcher_icon.rs`),
  e.g. for a wrapper script that `shell_icon` only sees as `Shell`.

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
- Notifications and close warnings:
  - Sound for agent events (Waiting, Ready for review): one bundled sound for both events
    (`rodio`). "Notification sounds" switch on the Notifications page, off by default (today
    is silent): off means no agent, long-command or bell sound, and no audio initialization;
    the volume slider stays, disabled, for when it is on. Visual status is unaffected.
    Parked: `rodio` needs the ALSA dev package (`libasound2-dev`) to build on Linux, also in CI.
