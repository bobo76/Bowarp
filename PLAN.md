# Agent Workspace - Implementation Plan

Feature spec: [FEATURES.md](FEATURES.md)

## Top priority: Ready for review

- Background agent completion status: when an unfocused agent tab transitions from Working
  to Idle, show **Ready for review** instead of Idle until the user focuses that tab.
  - Focusing the tab acknowledges the completed work and clears the status back to Idle;
    a Working-to-Idle transition in an already focused tab stays Idle.
  - This is an unread-completion indicator, not confirmation that the work is correct.
    New Working or Waiting events replace it with the agent's current status.
  - Show it consistently in the sidebar, tab hover cards and group summaries; align existing
    Done notifications with this status rather than introducing duplicate completion notices.

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
   - Close-with-running-process warnings: a tab is busy when a shell command runs (OSC 133
     `C` without `D`) or an agent is Working or Waiting; idle prompts and idle agents close
     silently. Applies to closing a tab, closing a group and quitting the window (one dialog
     listing every busy tab).
   - Sound for agent events (Waiting, Done): one bundled sound for both events, with an on/off
     switch and a volume setting. Basic silent in-app toasts are implemented, awaiting the
     manual checks below.
   - Clipboard notice: a banner above the terminal when a program writes the clipboard
     (OSC 52). The write is held until Allow (applied then) or Deny (dropped); a newer write
     replaces the pending one. "Always for this tab" allows later writes until the tab closes
     (not saved).
2. Configurable keyboard shortcuts: load a user keymap (keystroke -> action name, built with
   `cx.build_action`) from the settings file in place of `default_bindings`. First so the user
   can unbind Ctrl+V (paste) and get `^V` back for bash's quoted-insert and vim's visual block.
   - Overlay: `"keybindings": {"ctrl-v": null, "ctrl-shift-v": "Paste"}` adds to or overrides
     the defaults key by key; `null` removes a default, and the key then goes to the terminal.
   - Full editor on a "Keyboard" settings page: one row per action showing its keys, click to
     record a keystroke, a reset to the default, and a way to unbind it.
   - An invalid entry (unknown action, keystroke that can't be parsed) is skipped and the rest
     still apply; the reasons are listed on the settings page, like a broken settings file.
3. Configurable terminal path: the executable path for each launcher entry (Terminal, Git
   Bash, `claude`, `copilot`) saved in the settings file and shown on the settings page, for
   programs that aren't on `PATH` or where the wrong copy is found first; an empty value keeps
   the current lookup.
   - Every detected `+` menu entry gets one (PowerShell, cmd, Git Bash, zsh, `claude`,
     `copilot`, ...), except WSL distros, which are always started through `wsl.exe -d`.
   - A path that doesn't exist or can't be run: the settings row shows "(not found)", the entry
     uses the normal `PATH` lookup, and the tab shows a persistent warning, like the initial
     shell fallback.
   - Input: a text field with a Browse button (native file picker); the placeholder shows the
     path the `PATH` lookup finds now.
4. Terminal color schemes: a few bundled schemes plus user schemes from base16/iTerm files,
   picked on the Appearance page. "System" follows the OS light/dark setting. The persisted
   light/dark UI choice is implemented, awaiting manual checks below.
   - Two pickers: one scheme for the Light theme, one for Dark; the active UI theme (or the OS,
     for "System") decides which applies. The terminal scheme never colors the UI itself.
   - Bundled, written in code (no asset files): the current default (dark), a matching default
     light, Solarized Dark / Light, One Dark / Light, Dracula.
   - User schemes: base16 (YAML) and iTerm (`.itermcolors`) files placed in a `schemes/`
     folder next to `settings.json`, read when the Appearance page opens. The settings file
     stores only the chosen scheme names.
5. Window opacity and background blur, set in the settings file (opacity applies to the
   terminal background only, text stays opaque); blur where the platform allows it (Windows
   acrylic/mica, KDE on X11).
   - Also on the Appearance page: an opacity slider (30-100 %) and a blur dropdown, applied
     live and saved in the settings file.
   - Windows blur: None / Acrylic / Mica, where Mica falls back to Acrylic on Windows 10.
   - Without blur support (Wayland, GNOME on X11) the opacity still applies; the blur control
     is disabled and says "Not available on this system".
6. Major feature options: add the switches in [Feature options](#feature-options) to the
   settings page and settings file, alongside the existing appearance and terminal controls.
   These are planned options, not features already implemented.

### Feature options

UI/UX review recommendation: make major optional behavior configurable, rather than adding
a switch for every action. Defaults keep current or explicitly planned behavior; notification
sound starts off to keep today's silent default. Each row is an independent switch unless a
dependency is stated.

| Settings page | Option | Default | When off |
|---|---|---|---|
| Appearance | Tab hover cards (implemented; manual checks pending) | On | No cards over sidebar tabs; retain sidebar status. Keep the detailed field switches described under Fixed, not tested yet. |
| Appearance | Group hover cards | On | No basic or extended group cards; retain collapsed-group status. Independent of tab cards. |
| Appearance | Activity animations | On | Static Working indicators for both Claude and Copilot; no animation timers. Status still updates. |
| Terminal | Command history | On | Stop collecting commands into Bowarp history, in memory and on disk; no history popup, inline suggestions or last-command capture/restoration. Does not change the shell's own history. |
| Terminal | History popup | On | No Bowarp HISTORY panel or popup key interception; Up goes to the shell. With command history on, recording continues, stated beside this switch. |
| Terminal | Inline suggestions | On | No Bowarp ghost text or suggestion-acceptance key interception, including last-session suggestions; shell plugins are unchanged. |
| Terminal | Prompt at the bottom | On when built | Keep today's top-anchored terminal layout. Independent of history; with the history popup off, no docked panel is built. |
| Terminal | Command separators and details | On when built | No command-block separators, folder or duration labels; prompt placement is unchanged. |
| Terminal | Open agents typed at the prompt in a new tab | On when built | Run typed `claude` / `copilot` normally in the shell. Applies to newly started shells, as the integration functions are installed at startup. |
| Workspace | Restore previous tabs | On | Start with a fresh default tab; do not load or update the automatic tab snapshot. Keep ordinary preferences and explicitly saved named workspaces. |
| Workspace | Remember last shell command | On when built | Do not capture or save new `last_command` values or offer saved ones. Requires command history and automatic tab restoration; showing the suggestion also requires inline suggestions. |
| Workspace | File explorer and viewer | On | Hide both panes and disable their app entry points after resolving unsaved edits. File links offer Open externally; preserve pane widths and saved file associations without loading files. |
| Notifications | In-app notifications | On | No agent or long-command toasts; status and attention badges remain. Sound is independently controlled. |
| Notifications | Notification sounds | Off | No agent, long-command or bell sound; retain the volume slider for when enabled. Does not suppress visual status. |
| Notifications | Long-command completion status | On when built | Do not mark background shell tabs Done/failed or send completion notifications. Keep command tracking for busy-close protection; retain the duration threshold setting. |

- Persist switches in `settings.json`; missing or wrongly typed values use their documented
  defaults. Apply live except where noted. Show dependent controls disabled, retaining their
  values, with a caption explaining which parent option is required.
- Command history is the master switch for Bowarp's history features. Turning it off closes
  the popup and removes suggestions immediately, but does not disable shell integration used
  for working-directory tracking, agent status or busy-close protection.
- Distinguish disabling from deleting: existing history and saved session commands are not
  erased or used while their feature is off. Provide explicit Clear history and Clear saved
  session commands actions, covering both history storage and saved tab/workspace command
  fields where applicable; never rerun saved commands automatically.
- Disabled UI must stop work done solely for it: card clocks/diffs, suggestion searches,
  command-block metadata, toast queues, audio initialization, animations and file-pane
  scans/watchers. Keep shared work that another enabled feature still needs.
- Keep clipboard Allow/Deny, unsaved-edit confirmation, busy-close warnings and persistent
  failure warnings as safeguards, not optional notification switches. Theme, color schemes,
  opacity, blur, shell and icon choices remain pickers/sliders, not extra on/off switches.
- Integrate these controls with the existing notification next step and the Backlog items
  for hover cards, history per shell, prompt layout, last-command restoration, animations
  and named workspaces; do not implement a second set of settings for the same behavior.
- When implementing, cover settings defaults/round trips, parent-child combinations,
  live disable/re-enable, key pass-through, retained-data clearing and disabled background
  work in tests; manually check the settings UI and unsaved-edit flow.

## Fixed, not tested yet

Six UX improvements are implemented in 0.2.16. All 360 workspace tests, the build and Clippy
pass in Linux Docker. Native Windows builds and visual checks remain pending: the MSVC
linker is not available locally. See README's "Six UX improvements" manual checklist.

**Windows check, 2026-10-23 (build reported as 0.2.16): nothing passed.** The overflow menu,
the theme switch and the group card are all missing, though their code is in the `v0.2.16`
tag. So the build that was run probably doesn't contain these changes (an older build, or one
from an earlier commit). Before debugging, confirm which commit the Windows build came from,
then rebuild from `v0.2.16` and run all six checks again.

- Settings gear: focus an unfocused Settings page; close a focused page and remove its row.
  **Failed** (what goes wrong not recorded yet).
- File viewer overflow menu: existing Save and Raw/Rendered actions; toolbar controls stay.
  **Not shown:** no menu in the viewer.
- Initial shell fallback: a persistent warning retains the requested shell's failure reason.
  Not tested yet.
- Group hover card: whole-group tab count and Working/Waiting/Done totals, including empty
  groups; no timer, per-tab rows or card during dragging. **Not shown:** no card when
  hovering a group. (Dragging a group header doesn't work either, but that was never built:
  see the Backlog.)
- Background agent toasts: status-transition based, silent, in-app only; click to select,
  dismiss or auto-hide, replacing an earlier notice for the same tab. **Failed** (what goes
  wrong not recorded yet).
- Persisted Light theme switch: live UI changes, dark by default, terminal palette unchanged.
  **Not shown:** no theme setting on the settings page.

### Four requested features (unreleased, 2026-10-23)

Implemented with automated coverage; native Windows UI and live Copilot permission-dialog
checks remain pending. README documents the settings and behavior.

- Restore a shell tab's last command: integrated bash/zsh tabs (including Git Bash and bash
  in WSL; not PowerShell, cmd or agents) save the last non-trivial, recordable command at
  command start. `tabs.json` is owner-only on Unix at every write. The first empty prompt
  offers a one-shot dimmed suggestion with a fainter last-session hint when it fits; Right
  or End accepts, never Tab or Enter. Any line action dismisses it, including keys before
  the first prompt; HISTORY deletion clears it from all tabs.
  Manual check: run a useful command then `cd`/`ls`, restart and verify the useful command
  is suggested; check Right/End, Tab/Enter pass-through, dismissal, narrow-window hint,
  crash recovery and forgetting across tabs.
- Tab hover card settings: Appearance has a persisted master switch and Status, Location,
  Changes, Program title and Session kind switches, all on by default. Missing/wrong-typed
  fields default independently. Name/shortcut always remain; all pieces off suppresses the
  card without changing the master. Disabled piece controls retain their values. No card
  clocks when status is off and no new diffs when changes is off; group cards are unchanged.
  Manual check: toggle every field live, disable/re-enable the card, restart and confirm
  retained values; verify all-fields-off suppression and unchanged group cards.
- Copilot CLI NeedsAttention: tested against CLI 1.0.103's supported per-tab `--plugin-dir`.
  App-owned private plugins register only `PermissionRequest`, routed through `agentws-hook`
  to Waiting. Working/Idle still come from progress reports, which clear Waiting. User
  Copilot configuration is never changed; unavailable hooks keep progress-only fallback.
  Automatic approvals can briefly report Waiting because Copilot emits the hook first.
  Manual check: README checks 6-9 on Windows and Ubuntu (Wayland/X11), including two tabs,
  approval/denial, progress recovery, disabled hooks and unchanged user configuration.
- Agent launch arguments: Terminal shows Claude/Copilot paths and free-text argument fields
  (`claude_args`, `copilot_args`). Shell-style quoting groups words without shell expansion;
  malformed quoting blocks launch with an error. Arguments follow app hook arguments;
  user `--settings` is passed through even if it overrides hooks. Menu/explorer launches
  use current settings, while existing, duplicated and restored tabs keep original args.
  Ctrl+Shift+T continues opening the default shell.
  Manual check: launch with quoted arguments, empty fields, malformed quotes, Claude
  `--permission-mode plan`/`--model opus` and Copilot `--mode plan`/`--yolo`; change settings
  and confirm duplication/restoration keep the old arguments.

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

- Copilot working animation: show a blue circular spinner rotating beside the Copilot tab's
  title in the sidebar while its status is Working, matching the reference screenshot.
  Stop the animation when the status leaves Working; no animation timer while idle.
  - Copilot only: Claude tabs already have their own Working animation.
  - Same place as Claude's animation: just left of the title on the row's first line, where
    the half-circle glyph shows on a working Claude tab.
  - It animates only while the row can be seen: no frames while its group is collapsed or the
    window is minimized.
- Named workspaces (the single window's tabs are already restored): save and open named
  workspaces, offer to relaunch their tabs; restored tabs clearly marked "not running".
  - Opening one replaces every current tab, after a confirmation. No confirmation when the
    window has a single tab and it isn't busy (no command running, agent not Working or
    Waiting): that tab is replaced right away.
  - Each restored tab shows a "Not running" panel with a Start button (the open dialog also
    has "Start all"). Agent tabs start a new session (no `--continue`); shells start as new.
  - Saved: what `tabs.json` holds (kind, folder, names, groups, order, launch arguments),
    plus each tab's file open in the viewer and the viewer and explorer widths. Not the
    scrollback.
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
- New-tab folder picker (currently the selected tab's folder).
- File explorer: read folders off the UI thread (a "Loading..." row, results applied per
  root) so a folder with tens of thousands of entries or a slow network mount doesn't stall
  the window; Delete already runs in the background.
- Sidebar search: the filter/sort button from the reference screenshot.
- Configurable tab background color: a color picked per tab from the tab menu (a small
  palette plus "Default") that tints the tab's sidebar row and its terminal background; saved
  with the tab in `tabs.json` and restored. The terminal's foreground colors stay unchanged,
  so the palette must keep text readable in light and dark themes.
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

**Ideas to evaluate, not committed to yet:** not in [FEATURES.md](FEATURES.md) until picked.

- Diff panel: the selected tab's working-tree changes (`git diff` against `HEAD`, untracked
  files included) in the file viewer pane, one collapsible section per file with added and
  removed lines colored, opened from the hover card's `+N` or a title bar button, and
  refreshed when the file watcher sees a change. Shows what an agent changed without leaving
  the app; read-only (no staging or reverting).
- Git worktree and pull request on the sidebar row: a worktree marker when the tab's folder is
  a linked worktree (`.git` is a file), with the main repository in the hover card; the open
  PR's number and state for the branch, from `gh pr view --json` run off the UI thread and
  cached per branch, skipped when `gh` is missing or not logged in.

## Maybe

Ideas not committed to yet; not in [FEATURES.md](FEATURES.md) until picked.

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
