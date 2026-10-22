# Agent Workspace - Implementation Plan

Feature spec: [FEATURES.md](FEATURES.md)

## Status (0.2.16, 2026-10-22)

[FEATURES.md](FEATURES.md) is the target; what is not built yet is in the next steps and the
backlog below. Details of finished work are in the git history and [CHANGELOG.md](CHANGELOG.md).

Version 0.2.16 includes the six UX improvements below. Keep them under "Fixed, not tested
yet" until the native Windows UI checks have passed; publishing a version does not mark
those checks complete.

**Keeping this file current:** when something is completely done (built, tested and checked
where it has to be), remove it from this file; the git history keeps the record. Until then
it stays, under "Fixed, not tested yet" if only the manual check is missing.

**Known limitations**

- Not supported in the file viewer: AVIF images (GPUI's decoder lacks it: "The image could
  not be read."), a `#heading` in a document link (the file opens at the top).
- File viewer images: still WebP files share animated GIF/WebP's 2 MB limit (other images
  50 MB), and images over 16384 pixels on a side are refused rather than scaled down.
- An OSC 8 link spanning several lines: only its row is underlined.
- An agent tab going on as a shell starts in the agent's last known folder: on Linux its
  `/proc` folder as of its last title change or hook event, elsewhere the folder it started in.
- F11 has no menu entry: the app has no window menu (a right-click on the title bar opens the
  window manager's own).
- No command history in cmd tabs: cmd's prompt prints only the `D` mark, not the `A` / `B`
  marks that locate the prompt.
- Settings file: a field with a value of the wrong type gets its default silently and the
  value is dropped at the next change. A file that isn't a JSON object is ignored (reason
  printed and kept for the settings page) and moved to `settings.json.bak` at the first
  change. Unknown fields are kept but rewritten in alphabetical order; JSON comments aren't
  allowed. Edits made on disk while running apply when the settings page opens, when the
  file is saved in the file viewer, or at the next change from the app; edits made in another
  editor while the page shows apply only then (no file watch).
- Terminal font: a family that isn't installed (exact name; any case on Windows) silently uses
  the built-in one (DejaVu Sans Mono, Cascadia Mono on Windows); the page doesn't say so. So do
  GPUI's `.`-names (`.SystemUIFont`) and a localized or secondary family name (Linux lists only
  each font's first family name), even when the font would load under it. A font
  change shows in a tab's terminal when it is next drawn, so background tabs (and their pty
  size) catch up when selected. The slider covers 8-32 points; the file takes 6-72.
- Default shell (Terminal page): only the shells found at startup are offered (one installed
  while the app runs shows up at the next start). The choice is saved by its launcher id
  (`zsh`, `git-bash`, `wsl:Ubuntu`); the file also takes a menu name (`Zsh`). One that isn't
  found shows as "(not found)" and the automatic shell opens instead. A change applies to new
  tabs only: open and restored tabs keep the shell they started with. On Unix, when `$SHELL`
  isn't listed, the first tab runs it but Ctrl+Shift+T opens the first listed shell (the
  page then says just "Automatic").
- File viewer width: saved as a share of the window (`file_viewer_fraction`, 0-1 in the
  file; the 240 px minimums for the viewer and the terminal still apply) when a divider drag
  ends or the divider is double-clicked, not on the settings page. Another running instance
  shows it only after it reloads the settings file (see above). While the file is broken the
  width isn't saved (the file is left for the user to fix) and lasts until the file is read
  again. A drag whose release the app misses (a right or middle button released outside the
  window, or on Windows a focus change such as Alt+Tab mid-drag) keeps the dragged width and
  saves it at the next mouse release anywhere; it is lost only if the app quits first.
- File explorer width: dragged by the 6 px strip on the explorer's right edge (double-click
  for 300 px), kept between 150 and 600 px whatever the window size (a narrow window can
  leave the terminal and viewer below their usual minimums), and saved in pixels
  (`file_explorer_width`) with the same rules as the viewer width: once when the drag ends,
  not on the settings page, not while the file is broken, other instances after a reload.

**Next steps**

1. Notifications and close warnings:
   - Close-with-running-process warnings.
   - Configurable sound for agent events (Waiting, Done); basic silent
     in-app toasts are implemented, awaiting the manual checks below.
   - Clipboard notice: a banner above the terminal when a program writes the clipboard
     (OSC 52), with allow/deny.
2. Configurable keyboard shortcuts: load a user keymap (keystroke -> action name, built with
   `cx.build_action`) from the settings file in place of `default_bindings`. First so the user
   can unbind Ctrl+V (paste) and get `^V` back for bash's quoted-insert and vim's visual block.
3. Configurable terminal path: the executable path for each launcher entry (Terminal, Git
   Bash, `claude`, `copilot`) saved in the settings file and shown on the settings page, for
   programs that aren't on `PATH` or where the wrong copy is found first; an empty value keeps
   the current lookup.
4. Agent launch arguments: extra command-line arguments for the Claude Code and Copilot CLI
   launcher entries (e.g. `--yolo` for `copilot`, `--dangerously-skip-permissions` or
   `--model opus` for `claude`), saved in the settings file and shown on the settings page
   next to the entry's path. Split like a shell command line (quotes group words) and added
   after the app's own arguments (`claude` keeps the hooks' `--settings`); they apply to new
   tabs started from that entry (the `+` menu, Ctrl+Shift+T). A restored tab keeps the
   arguments it started with (saved with the tab in `tabs.json`), not the current setting.
   Empty adds none.
   - Claude Code mode: `--permission-mode <default|acceptEdits|plan|bypassPermissions>`
     (`--dangerously-skip-permissions` is `bypassPermissions`); `permissions.defaultMode` in
     the hooks' `--settings` file would also work. The hook input's `permission_mode` reports
     the current mode, but only when a hook fires (a Shift+Tab switch while idle shows at the
     next event).
   - Copilot CLI mode: `--mode autopilot` (or `--autopilot`), `--mode interactive`, or
     `--mode plan`. Mode selection is separate from permission bypass (`--yolo`).
5. Terminal color schemes: a few bundled schemes plus user schemes imported from base16/iTerm
   files, saved in the settings file and picked on the Appearance page. "System" follows the
   OS light/dark setting. The persisted light/dark UI choice is implemented, awaiting manual
   checks below.
6. Window opacity and background blur, set in the settings file (opacity applies to the
   terminal background only, text stays opaque); blur where the platform allows it (Windows
   acrylic/mica, KDE on X11).

## Fixed, not tested yet

Six UX improvements are implemented in 0.2.16. All 360 workspace tests, the build and Clippy
pass in Linux Docker. Native Windows builds and visual checks remain pending: the MSVC
linker is not available locally. See README's "Six UX improvements" manual checklist.

- Settings gear: focus an unfocused Settings page; close a focused page and remove its row.
- File viewer overflow menu: existing Save and Raw/Rendered actions; toolbar controls stay.
- Initial shell fallback: a persistent warning retains the requested shell's failure reason.
- Group hover card: whole-group tab count and Working/Waiting/Done totals, including empty
  groups; no timer, per-tab rows or card during dragging.
- Background agent toasts: status-transition based, silent, in-app only; click to select,
  dismiss or auto-hide, replacing an earlier notice for the same tab.
- Persisted Light theme switch: live UI changes, dark by default, terminal palette unchanged.

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

| Element | Look | Where |
|---|---|---|
| Context menu | Save as new config (= save as a named workspace) | Backlog: named workspaces |
| Clipboard notice | banner above the terminal when a program writes the clipboard (OSC 52), with allow/deny | Next step 1 |

"Share session" from the screenshot is not planned.

## Backlog

- Named workspaces (the single window's tabs are already restored): save and open named
  workspaces, offer to relaunch their tabs; restored tabs clearly marked "not running".
- Copilot CLI NeedsAttention via its hooks (progress reports only give Working / Idle;
  Copilot's `cli-main.js` has a `hooks` config and a `PermissionRequest` event: per-tab hooks
  config like `ClaudeHooks`, event through `agentws-hook` to `agent-status`, fall back to the
  progress status without hooks; pin the Copilot version tested). Re-run the manual checks 6-9
  (README) on Ubuntu, Wayland and X11.
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
  - Setting "Open agents typed at the prompt in a new tab" on the settings page's Terminal page
    (default on); off, the functions aren't defined (the app passes a flag with the token).
  - A `claude` run in place (`-p` in a script, a nested shell, cmd tabs, which get no functions)
    gets no hooks and shows "Running for" until it quits. Accepted.
- Interrupting Claude Code (Esc) leaves the tab Working until the `idle_prompt` notification
  (~60 s); no hook fires on interrupt.
- Bell sound: with the M5 notification sound; rate-limited so a stream of bells doesn't
  repeat it (the bell already marks a hidden tab Needs attention).
- Long command finished in a shell tab: when a command that ran longer than a threshold
  (default 10 s, in the settings file) ends while its tab isn't selected, the tab is marked
  done, or failed when the OSC 133 `D` exit code isn't 0, until it is viewed. Needs shell
  integration. Same notification and sound as M5's agent notifications.
- Command palette: Ctrl+Shift+P opens a fuzzy search over every action (the ones with
  shortcuts and the menu-only ones such as Duplicate, Move to group or Copy branch), showing
  each one's shortcut, plus the tabs by name ("go to tab"). Enter runs the action on the
  selected tab.
- File viewer find and replace: Ctrl+F searches the open file (highlight all matches,
  Enter / Shift+Enter to move, match count, case and whole-word toggles), Ctrl+H adds a
  replace field (replace one / all) in source mode; rendered Markdown only gets find.
- File viewer: the rendered JSON view ("Not valid JSON") is only worked out when the file
  opens, so it stays stale after the file is fixed and saved in the viewer.
- Command history with zsh-autosuggestions (likely bug, unverified): Enter on a HISTORY row
  sends Ctrl+E, which the plugin binds to accept the grey suggestion, but the reported line
  (`$BUFFER`, `.zshenv` `__agentws_report_line`) excludes it, so the Backspaces leave the
  accepted rest in front of the new command. Send a key that doesn't accept it, or clear
  `POSTDISPLAY` first.
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
  4. Setting "Prompt at the bottom" on the settings page's Terminal page (default on); off
     keeps today's top-anchored layout.
  Not planned from the screenshot: the separate input box with the folder chip (the prompt is
  the shell's own, in the grid) and the panel's "Configure" button.
- Zoom with Ctrl+scroll, like Windows Terminal: Ctrl+wheel up / down over the terminal makes
  the font bigger / smaller one point per notch (clamped, e.g. 6-48 pt), also Ctrl+= / Ctrl+-
  and Ctrl+0 to reset. Today the size is the hard-coded `FONT_SIZE` (14 pt,
  `app/src/terminal_element.rs`); builds on Next steps 1 (font size from the settings file,
  applied live by re-measuring the cells and resizing the pty). The zoom is per tab: it changes
  only the tab under the mouse (or the focused one, for the keys), and a new tab starts at the
  settings size, which acts as the default zoom. Zooming never changes the setting and isn't
  saved (Ctrl+0 and a restart go back to the settings size). Ctrl+wheel is never sent to the
  program, even with mouse reporting on.
- Restore a shell tab's last command: each shell tab (shell integration only; not agent tabs,
  not cmd tabs, which report no commands) keeps the last command it ran, saved with the tab in
  `tabs.json` (a `last_command` field in `SavedTab`, written when the window closes like
  `folder`). Taken from the `ExecutedCommand` that `TerminalView` already passes to
  `history::record`, so unrecordable commands (leading space, control characters) are never
  saved. When the restored tab shows its first prompt with an empty line, the command is drawn
  as the dimmed inline suggestion and accepted with the same keys (Right, End, Tab); it is
  never typed into the line, so an Enter can't rerun `git push` or `rm` by accident. It goes
  away once something is typed (the normal history suggestion takes over) or a command runs.
  `tabs.json` must then be written owner-only like `history.jsonl` (commands may hold
  secrets). Unit-test the `SavedTab` round trip (field missing, present) and the empty-line
  suggestion.
- New-tab folder picker (currently the selected tab's folder).
- File explorer: read folders off the UI thread (a "Loading..." row, results applied per
  root) so a folder with tens of thousands of entries or a slow network mount doesn't stall
  the window; Delete already runs in the background.
- Sidebar search: the filter/sort button from the reference screenshot.
- Configurable tab background color: a color picked per tab from the tab menu (a small
  palette plus "Default") that tints the tab's sidebar row and its terminal background; saved
  with the tab in `tabs.json` and restored. The terminal's foreground colors stay unchanged,
  so the palette must keep text readable in light and dark themes.
- Tab hover card settings: a "Tab hover card" switch (default on) on the settings page that
  turns the card on the sidebar rows off or on, and one switch per piece of information it
  shows (all on by default), saved in the settings file:
  - Status: the colored chip, with the lap time ("Working · 2m 14s") or the running command,
    and the "Running for 2:14" line.
  - Shortcut: the tab's Ctrl+number shortcut.
  - Folder: the full folder, home shortened to `~`.
  - Branch: the git branch.
  - Program title: the program's title, when the row's custom name hides it.
  - Session kind: the kind icon and name (or the shell's name).
  - Lines added: the `+N` from `git diff`.
  Applies live. Card off: `with_hover_card` returns the bare row, so no card is built, no
  `git diff` runs and no clock ticks. Lines added off: `refresh_diff_stat` doesn't run
  `git diff` either. Every piece off is the same as the card off. `sidebar::tab_card` skips
  the unchecked parts (a `TabCardFields` set in `workspace-model/src/settings.rs`, read in
  `tab_card.rs`); unit-test the settings read (missing, wrong type) and which parts a set
  keeps.
- Extended group hover card (basic counts are implemented): one line per tab with kind icon,
    name, status and lap time ("Working · 2m 14s", "Running for 40s") and branch, in sidebar
    order; tabs that need attention (Waiting, unviewed Done or failed) first.
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
    GPUI / `gpui-component`), `windows-registry` 0.6 -> 0.100 (needs Rust 1.95). Update the
    Crates table above afterwards.
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

## Maybe

Ideas not committed to yet; not in [FEATURES.md](FEATURES.md) until picked.

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

## Risks

- **GPUI API churn:** exact `gpui-pre` version; UI isolated in `app`.
- **IME and clipboard edge cases** in a custom terminal element: test early in M1, especially on Windows.
- **Claude Code `--settings` flag and hook payload shapes:** verify against current docs before relying on them.
