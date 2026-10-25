# UX/UI review

Date: 2026-10-25

Scope and method: a review of the whole app against README.md, FEATURES.md, PLAN.md,
docs/known-limitations.md, docs/ux-decisions.md (not reopened unless stated), the Warp reference
screenshots in `docs/ui-reference/`, and targeted reads of `crates/app/src/` and
`crates/workspace-model/src/`. Live screenshots were taken of the debug build (0.2.30, 9bc7699)
under Xvfb with the `pipeline/tools/` harness (1200x800, then 640x480; scratch config, stand-in
`claude` / `copilot`, a fake permission hook): start, `+` menu, four tabs, hover card, tab
menu, group create/collapse, attention toast, explorer, file viewer, Settings (Appearance,
Terminal, Keyboard), About, exited tab, terminal search. They are cited below as "shot NN"
and were not added to the repo. Not seen live (no window manager / compositor under Xvfb):
title bar window controls, maximize, opacity on a real desktop, the light theme, Windows.

Items already planned in PLAN.md (command palette, light theme softening, renamed Claude
spinner, viewer close shortcut and search, new-tab folder picker, async explorer reads,
TerminalView caching, extended group card, group drag) are not repeated.

## Summary

| # | Area | Suggestion | Impact | Cost | Needs the user |
|---|---|---|---|---|---|
| 1 | Layout | Hide the file viewer while Settings shows | High | S | no |
| 2 | Layout | Viewer width from the free space; terminal keeps ~60 columns | High | S | no |
| 3 | Layout | Make the sidebar resizable, saved like the explorer width | Medium | M | no |
| 4 | Sidebar | Name shell tabs by command / folder, not `user@host: path` | High | S | yes |
| 5 | Sidebar | Draw a group rail so group membership is visible | Medium | S | no |
| 6 | Sidebar | Tell a failed exit from a clean one on the row | Medium | S | no |
| 7 | Sidebar | Compact row density for many tabs | Medium | M | yes |
| 8 | Sidebar | Label the hover card's `+N` | Low | S | no |
| 9 | Sidebar | No hover card while a rename editor or menu is open | Low | S | no |
| 10 | Sidebar | Prompt redraws shouldn't count as "Recent output" | Low | S | no |
| 11 | Attention | Taskbar flash / urgency when unfocused and a tab needs you | High | S | yes |
| 12 | Attention | Window title follows the selected tab and attention count | Medium | S | no |
| 13 | Attention | Say what the agent waits for in the toast, with the jump key | Low | S | no |
| 14 | Errors | Show "agent status disabled" in the UI, not only on stderr | High | S | no |
| 15 | Errors | Start failures open the window with an error, not exit | High | S | no |
| 16 | Errors | Say when a folder can't be watched (explorer/viewer go stale) | Low | S | no |
| 17 | Terminal | Restart an exited tab (button + Enter) | Medium | S | no |
| 18 | Terminal | Close a shell tab on a clean `exit` | Medium | S | yes |
| 19 | Terminal | One-time "Hold Shift to select" hint under mouse reporting | Low | S | no |
| 20 | Tabs | Reopen closed tab | Medium | M | yes |
| 21 | Persistence | Closing the last tab shouldn't wipe the saved workspace | High | S | yes |
| 22 | Persistence | Restored agent tabs: say it's a new session, start on demand | High | M | yes |
| 23 | Explorer | Keyboard navigation in the file tree | Medium | M | no |
| 24 | Explorer | Dim git-ignored entries | Low | M | no |
| 25 | Settings | Narrow the settings nav, keep inputs inside the row | Medium | S | no |
| 26 | Settings | Shorten row descriptions to one sentence | Medium | M | no |
| 27 | Settings | Say when the font family isn't installed | Medium | S | no |
| 28 | Settings | Wording: "Bowarp", Tab vs last-session text, Keyboard subtitles | Low | S | no |
| 29 | Shortcuts | All shortcut hints in menus/tooltips; "Copy title" | Low | S | no |
| 30 | Shortcuts | Actions for rename, pin, move tab, previous tab | Medium | S | no |
| 31 | Shortcuts | Ctrl+9 selects the last tab | Low | S | yes |
| 32 | Title bar | Drop the `i` About button (Settings -> About stays) | Low | S | yes |
| 33 | A11y | Give Running a shape, not only a teal color | Low | S | no |
| 34 | A11y | A left accent bar on the selected row | Low | S | no |

## Layout, dividers and narrow windows

### 1. Hide the file viewer while Settings shows
- Problem: the viewer stays beside the Settings page, which then gets what is left. At 1200 px
  with the explorer and README.md open, Settings is ~240 px wide, one word per line (shot 08).
  `Render for WorkspaceView` filters the terminal by `shows_page()` but not `viewer`
  (`workspace_view.rs:1762-1767`).
- Recommendation: while `shows_page()`, don't render the viewer or its divider (the tab keeps
  its open file and unsaved edits; it reappears with the tab). One `.filter`, because the
  viewer belongs to the tab that Settings replaces.
- Edge cases: "Open settings file" opens `settings.json` in the viewer: it must then close
  the page (or the file opens out of sight). The explorer also keeps the hidden tab's
  folder under Settings; keep it (it is a global toggle), but no cd actions apply while
  Settings shows.

### 2. Viewer width from the space right of the fixed panes; a real terminal minimum
- Problem: `viewer_width = fraction * row` uses the whole window (`pane_width.rs:21`), so with
  the sidebar (240) and explorer (300) the 45 % viewer leaves the terminal its 240 px minimum
  (`MIN_TERMINAL_WIDTH`, `pane_width.rs:14`): ~28 columns, the prompt wraps (shot 07). The
  terminal is the main thing and loses first.
- Option A — Fraction of `row - fixed` (sidebar + explorer), terminal minimum raised to ~60
  columns (cell width x 60, or 480 px). Pros: the terminal keeps a usable width; the saved
  fraction keeps its meaning. Cons: the viewer is narrower with the explorer open.
- Option B — Auto-hide the explorer when opening a file would push the terminal under the
  minimum. Pros: VS Code-like "make room". Cons: a pane vanishing on a click surprises.
- Option C — Leave it. Cons: shot 07.
- Recommendation: A, because it is a pure-function change in `workspace-model` (unit-testable)
  and keeps the terminal readable.
- Edge cases: a window too narrow for everything: viewer minimum (240) still wins over the
  terminal minimum, as today; at 640x480 suggest the sidebar toggle in the README (no auto
  hiding). Saved `file_viewer_fraction` values keep loading (meaning shifts slightly).

### 3. Resizable sidebar
- Problem: the sidebar is a fixed 240 px (`sidebar.rs:25`) while the explorer and the viewer
  can be dragged. Titles truncate after ~16 characters ("PLAN.md rema…", shot 03); at 640 px it
  takes 37 % of the window (shot 13).
- Recommendation: the same 6 px edge strip as the explorer, 180-400 px, double-click resets
  to 240, saved as `sidebar_width` with the explorer width's rules (known-limitations). Reuse
  `ExplorerWidth` / divider-drag code rather than a new pattern.
- Edge cases: hover cards and the drag preview use `SIDEBAR_WIDTH` (`sidebar.rs:237`) and
  must follow the live width; the attention button and `+` must stay visible at 180 px.

## Sidebar and tab rows

### 4. Shell tab names that tell tabs apart (Needs the user)
- Problem: a shell row shows the program's title, `user@host: ~/path` for bash, truncated to
  "bobo@boboBee…"; two shells in different folders look identical (shot 03), and the second
  line already shows the branch / folder. `tab_name` falls back to the title
  (`workspace_view.rs:967-971`).
- Option A — With shell integration: the running command while one runs, else the folder's
  last component (`agent-workspace`); the program title moves to the hover card (its
  "Program title" piece exists). Pros: rows differ and say what is happening; uses OSC 133 data
  already tracked. Cons: differs from Windows Terminal's title-as-name; shells without the
  integration still show titles.
- Option B — Strip a leading `user@host:` from titles. Pros: tiny. Cons: heuristic, prompt
  formats vary (zsh themes, PowerShell).
- Option C — Keep; users rename. Cons: rename is mouse-only today (see #30).
- Recommendation: A, because it reuses the integration's command and folder and fixes the most
  visible row text; needs the user since it changes what every shell row says.
- Edge cases: a custom name always wins; agent tabs keep their titles; long commands truncate
  at the end; `ssh` shows "ssh host" while it runs (correct).

### 5. Group rail
- Problem: grouped rows differ from ungrouped ones only by a 12 px indent; the ungrouped row
  right after a group looks like part of it (shot 16, "Copilot CLI" under "Agents").
- Recommendation: a 2 px vertical line from the group header to its last row, in the group's
  color (muted border color when none), at the chevron's x. Like VS Code indent guides and
  Chrome's colored group underline; no new interaction.
- Edge cases: collapsed groups draw nothing; search shows the rail on matched rows only; the
  drop marker must stay visible over it.

### 6. Failed exit vs clean exit
- Problem: Exited is a hollow ring in `#545d68` (`sidebar.rs:93`), the same grey as Quiet,
  hard to see on the dark sidebar (shot 18: a shell exited with code 3 looks idle). A failure
  reads exactly like a clean end.
- Recommendation: nonzero or unknown code -> a red ring with a small `×` and "Exited (3)" in
  the hover card status; code 0 keeps the grey ring, raised to >= 3:1 contrast. State stays
  shape plus color plus card text.
- Edge cases: agent tabs that go on as a shell never reach Exited on code 0; a group header's
  counts could add "failed" later (not needed now).

### 7. Compact rows for many tabs (Needs the user)
- Problem: rows are 57 px (two lines + 32 px tile), ~12 tabs fit at 800 px; with groups and
  agents in many folders users scroll, and Ctrl+1..9 reaches only 9.
- Option A — "Row density: Comfortable / Compact" picker on Appearance; compact is one 32 px
  line (icon with badge, name), branch/folder in the card. Pros: 2x tabs on screen. Cons: a new
  setting; the second line (FEATURES "Show each tab's git branch...under its name") goes.
- Option B — Auto-compact when the list overflows. Cons: rows change shape as tabs open.
- Recommendation: A, as a picker (allowed by FEATURES "Optional behavior"); needs the user
  because compact contradicts "under its name".

### 8. Label `+N` in the hover card
- Problem: the card shows a bare green "+5" (`sidebar.rs:712`, shot 04): lines? files?
- Recommendation: "+5 lines" (or a tooltip "Lines added in the working tree"). One string.

### 9. No hover card over editors and menus
- Problem: a tab card popped up over the screen while the new group's name was being typed
  (shot 15); at 640 px a card covers the prompt (shot 13).
- Recommendation: don't open a card while a rename editor, a context menu or a dialog is open,
  same as the existing "no card during a drag" rule.

### 10. "Recent output" for a shell at its prompt
- Problem: a selected bash tab sitting at its prompt shows "Recent output" (shot 13): the
  prompt redraw counts as output.
- Recommendation: in integrated shells, output between the `D`/`A` and `B` marks (the prompt
  itself) doesn't refresh Active. Low priority; cmd and non-integrated shells keep today's rule.

## Status, attention and notifications

### 11. Ask the OS for attention when the window isn't focused (Needs the user)
- Problem: a background agent waiting for permission is only visible inside the app (badge,
  triangle, toast). If the user is in a browser or IDE, nothing tells them; README:302 says
  "There is no sound or OS notification". GPUI already has `Window::request_attention`
  (Windows taskbar flash, X11 urgency hint; a no-op on Wayland) and
  `App::show_system_notification`.
- Option A — `request_attention()` when any tab turns Needs attention (or Ready for review)
  while the window isn't active. Pros: the native, quiet signal of every terminal and IDE; a
  few lines; no polling. Cons: nothing on Wayland.
- Option B — A + a system notification ("PLAN.md remaining work needs attention"), behind
  the existing "In-app notifications" switch or a new one. Pros: also works on Wayland/GNOME.
  Cons: OS notification permissions and noise; contradicts the README line.
- Option C — Keep in-app only.
- Recommendation: A now (no setting: it follows "In-app notifications"), B left to the user.
- Edge cases: clear the flash/urgency when the window activates (X11 clears it on focus);
  several instances flash independently; Settings shown counts as background (as for toasts).

### 12. Window title follows the selected tab
- Problem: the OS title is always "Agent Workspace" (`workspace_view.rs:325`); two instances
  are indistinguishable in Alt+Tab and the taskbar.
- Recommendation: "<tab name> — Agent Workspace", prefixed "(2) " while tabs need attention
  (like web mail). Set it on selection, rename and status change only (no redraw cost).
- Edge cases: Settings shown -> "Settings — Agent Workspace"; a very long tab name is cut
  (taskbars truncate anyway).

### 13. Toast says what the agent waits for
- Problem: the toast reads "Needs attention" (shot 14) for a permission prompt, a question
  and a bell alike; it doesn't mention Ctrl+Shift+A.
- Recommendation: keep the hook's `notification_type` and say "Waiting for permission" /
  "Waiting for your reply"; bell and clipboard keep "Needs attention". Clicking already selects
  the tab; add the jump key as a tooltip on the toast. Needs `agent-status` to carry the kind.

## Error states (Windows release builds have no stderr)

### 14. Agent status disabled must show in the UI
- Problem: when `agentws-hook` is missing next to the exe or the hook server can't start,
  `ClaudeHooks::start` fails (`hooks.rs:34-39`) and `main.rs:127` prints to stderr only. On a
  Windows release build Claude tabs then never show Working / Waiting, and the user reads
  silence as idle (FEATURES: "Do not treat ... terminal silence as proof that it is idle").
- Recommendation: the persistent warning used for the shell fallback ("Agent status is off:
  agentws-hook.exe not found next to the app. Reinstall to fix.") and the agent tabs' card
  status "Status unavailable" instead of Idle/No recent output.
- Edge cases: shell-only users get the warning once per start; dismissing hides it for the run.

### 15. Command-line and first-tab failures open a window
- Problem: `main.rs:56` (bad arguments), `main.rs:113` (folder not a directory) and
  `main.rs:310` (shell and fallback both fail) print to stderr and exit. On Windows a pinned
  shortcut with a removed folder looks like "the app doesn't start".
- Recommendation: a missing folder opens the tab in the home folder with an error toast naming
  the folder; a total start failure opens the window with an error panel in the terminal area
  (message, "Open settings" to change the default shell, Retry). Bad arguments: same panel with
  the usage text. Keep the stderr lines for Linux.

### 16. Say when a folder can't be watched
- Problem: a watcher error only logs (`folder_watcher.rs:63`, `file_viewer.rs:413`); the tree
  and viewer then silently stop following disk changes (network mounts, inotify limits).
- Recommendation: a muted "Not following changes" caption with a refresh button in the
  explorer / viewer header. No polling.

## Terminal view

### 17. Restart an exited tab
- Problem: the exit bar says "Process exited with code 3" and "Close the tab (Ctrl+Shift+W)"
  (shot 18); the only way to get the same tab back (name, color, group, folder) is a new tab
  plus re-arranging. The block cursor stays drawn on the dead grid.
- Recommendation: a "Restart" button on the bar and Enter (while the tab has focus) restarts
  the same launch spec in the tab's last folder, as Windows Terminal does ("[Enter] to
  restart"). Hide the cursor once the process is gone.
- Edge cases: agent tabs restart with their saved args (no `--continue`); scrollback is
  cleared on restart (state it on the button tooltip).

### 18. Close a shell tab on a clean `exit` (Needs the user)
- Problem: typing `exit` in a shell leaves a dead tab with an exit bar; most terminals close
  the tab on a clean exit (Windows Terminal `closeOnExit: graceful`, GNOME Terminal).
- Recommendation: shell tab, code 0 -> close the tab (it is not busy, so no warning); nonzero
  -> keep the bar (#17). Needs the user: it changes README "When the program exits, a bar...".
- Edge cases: the last tab follows #21; agent tabs keep "go on as a shell".

### 19. "Hold Shift to select" hint
- Problem: in Claude Code (mouse reporting) a drag selects nothing; the Shift override is
  only in the README.
- Recommendation: on the first drag in a mouse-reporting tab per run, a short info toast "Hold
  Shift to select text". One flag, no timers beyond the toast's.

### 20. Reopen closed tab (Needs the user)
- Problem: a closed tab (accidental Ctrl+Shift+W, "Close other tabs") loses its name, color,
  group, pin and folder; the Warp reference has "Reopen closed session" (new-tab-menu.png).
- Recommendation: keep the last 10 closed tabs' `tabs.json` entries in memory; a "Reopen
  closed tab" entry at the bottom of the `+` menu, an unbound `ReopenClosedTab` action
  (Ctrl+Shift+T is New tab; Ctrl+Alt+T is GNOME's terminal key), later the palette. It starts
  a new session in that folder and place. New feature, so it needs the user.

## Persistence and restore

### 21. Closing the last tab (Needs the user)
- Problem: closing the last tab closes the window (`workspace_view.rs:921-923`) after saving a
  snapshot with no tabs (`close_window`, `workspace_view.rs:733-735`), so the next start has
  lost every group, empty group, color and pin.
- Option A — Keep the window with an empty state: "New tab" buttons for the launchers, "Reopen
  closed tab" (#20), and the groups still listed. Pros: the workspace survives. Cons: a
  window that doesn't close on the last Ctrl+Shift+W (VS Code behaves so; Windows Terminal
  doesn't).
- Option B — Close the window but keep the previous snapshot when the last tab goes. Pros: no
  new UI. Cons: a restart restores tabs the user closed on purpose.
- Option C — Today's behavior.
- Recommendation: A, because the organization is the app's main value; needs the user.

### 22. Restored agent tabs: new session, started on demand (Needs the user)
- Problem: a restored Claude tab named "PLAN.md remaining work" starts a fresh `claude` with no
  hint that the conversation is gone, against FEATURES:109 ("Clearly distinguish restoring a
  layout from ... an agent's conversation"); every restored agent starts at once (CPU, memory
  and startup time with many tabs).
- Option A — Agent tabs restore "Not started": a panel "New Claude Code session in <folder>.
  Enter: Start · Start with --continue"; it starts when the user presses Enter/clicks (not on
  mere selection). Shells start as today. Pros: honest; cheapest startup; reuses the parked
  named-workspaces panel. Cons: one extra key per agent tab.
- Option B — Start all, plus a dim first line in the terminal area "New session — the earlier
  conversation is not resumed (claude --resume)". Pros: no extra key. Cons: costs stay.
- Option C — Restore with `--continue`. Cons: picks the folder's latest conversation, which
  may be another tab's.
- Recommendation: A; needs the user (startup behavior and FEATURES "starting fresh sessions").

## File explorer and viewer

### 23. Keyboard navigation in the tree
- Problem: the explorer has no focus handle and only an Escape action
  (`file_explorer.rs:609`): every action is mouse-only, against "keyboard first".
- Recommendation: a focusable tree: Up/Down move, Right/Left expand/collapse, Enter opens,
  F2 rename, Delete asks, Shift+F10 / Menu key opens the entry menu; a visible focus row;
  Ctrl+Shift+| opening the explorer focuses it, Esc returns to the terminal. Keys are local
  to the tree, so no terminal key is taken.

### 24. Dim git-ignored entries
- Problem: `target/`, `.git/` and the sources look the same (shot 06); in Rust/Node repos
  the big ignored folders sit among the real ones.
- Recommendation: dim entries git ignores (one `git check-ignore --stdin` per expanded folder
  off the UI thread, refreshed with the watcher). Folder order unchanged.

## Settings

### 25. Narrower settings nav; inputs inside the row
- Problem: the Settings nav column is ~360 px (gpui-component's resizable range is 160-360),
  next to the 240 px app sidebar: 600 px of navigation; the Font family input runs past the
  window's right edge at 1200 px (shot 09).
- Recommendation: `SettingsView::sidebar_width(px(200.))` and a max width on text inputs so
  they stay inside the content column.

### 26. One-sentence descriptions
- Problem: rows carry README paragraphs ("Command history": 7 lines, shot 11); the page is
  hard to scan and long. Search keywords already cover findability.
- Recommendation: one sentence per row (what it does + what off means); the details in a
  tooltip on an info icon, or in the README. Keep the disabled-reason captions.

### 27. Font not installed
- Problem: a missing family silently uses DejaVu Sans Mono / Cascadia Mono
  (known-limitations "Terminal font"); the row gives no feedback.
- Recommendation: under the field, the existing caption style: "Not installed; using DejaVu
  Sans Mono". Option: a dropdown of installed monospace fonts (more work; free text stays for
  `.`-names).

### 28. Wording fixes
- "Bowarp's history" in two descriptions (`settings_page.rs:359`, `:395`): the UI says Agent
  Workspace everywhere else.
- "Inline suggestions ... accepted with Right, End or Tab, including the last session's
  command" (`settings_page.rs:383-385`) vs README:161 "Right or End accepts it; Tab keeps
  shell completion": one of them is wrong; check the code and align both.
- Keyboard page: '"Copy" in settings.json.' under every action (shot 10) is noise; move the
  action id to the row's tooltip.

## Shortcuts, menus and discoverability

### 29. Shortcut hints everywhere they apply
- Tab menu: "Duplicate tab" shows Ctrl+Shift+D (`tab_menu.rs:75`) but "Close tab" doesn't show
  Ctrl+Shift+W (`tab_menu.rs:109`, shot 05).
- "Copy pane title" (`tab_menu.rs:90`): the app has no panes; "Copy title".
- Explorer close tooltip "Close explorer" lacks Ctrl+| (`file_explorer.rs:646`).
- "Search tabs..." placeholder: add the key in a tooltip ("Search tabs (Ctrl+Shift+S)").
- All through `shortcuts::with_shortcut` / `menu_label_with_hint`, so rebinding stays right.

### 30. Actions for frequent tab gestures
- Problem: rename, pin, move up/down and "back to the previous tab" have no action, so the
  Keyboard page can't bind them and the palette (PLAN) will have nothing to call.
- Recommendation: add `RenameTab`, `TogglePin`, `MoveTabUp`, `MoveTabDown`, `PreviousSelected`
  (MRU toggle) actions. Defaults: Ctrl+Shift+PageUp / PageDown for move (Chrome, Firefox;
  not used by shells or TUIs), the rest unbound (F2 would take htop/mc keys).
- Edge cases: move across group boundaries behaves like the menu's Move up/down.

### 31. Ctrl+9 = last tab (Needs the user)
- Problem: Chrome, VS Code and Windows Terminal map Ctrl+9 to the last tab; here it is the 9th.
- Recommendation: Ctrl+9 = last tab in sidebar order; Ctrl+1..8 unchanged. Changes README.

### 32. Drop the title bar About button (Needs the user)
- Problem: the `i` button takes one of four title bar slots for a rare action; Settings
  already has an About page and the version is copied from there.
- Recommendation: remove the button; Settings -> About keeps the dialog's content (FEATURES
  "About dialog" is then the page). Needs the user (removes an entry point).

## Accessibility and theming

### 33. Running needs a shape
- Problem: Working `#4c9aff` and Running `#2dd4bf` (`sidebar.rs:89-92`) are both blue-green
  12 px dots; Running pulses only for 10 s, and Activity animations off removes even that.
  Hard to tell apart with tritan color vision or at a glance.
- Recommendation: a small inner dot or ring on the Running badge (the badge already uses a
  check for Ready and a ring for Exited). Doesn't reopen the spinner/contrast decisions.

### 34. Selected row accent
- Problem: the selected row is only a slightly lighter background (shots 03, 16); it reads
  weakly in the dark theme and the light theme's softening (PLAN) may weaken it further.
- Recommendation: a 2 px accent bar at the row's left edge (on the left of the 3 px color
  stripe when the tab has a color), in the theme's accent color.

## Quick wins

In priority order (high impact or near-zero cost, small effort):

1. #1 Hide the viewer while Settings shows.
2. #14 Agent status disabled shown as a persistent warning.
3. #15 Start failures open the window with an error instead of exiting.
4. #2 Viewer width from the remaining space, terminal minimum ~60 columns.
5. #11 A `request_attention()` when unfocused (after the user's yes).
6. #12 Window title follows the selected tab.
7. #6 Failed exit shown on the row.
8. #29 Missing shortcut hints and "Copy title".
9. #28 Wording fixes.
10. #25 Narrower settings nav.
