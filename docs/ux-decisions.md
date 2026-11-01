# UX/UI decisions

> **Keep this file minimal.** One short entry per decision: the choice, why, and the main
> rejected alternative only when it explains the choice. Don't copy what the code already
> says (values, colors, timings, function names, `file:line`): reference the file or sum it
> up in a few words. Merge into or replace an earlier entry when a decision changes it,
> rather than appending "superseded" notes. Who decided is in brackets; "user" decisions are
> only reopened by the user.

## Keyboard

- **Recording refuses terminal control keys** (Ctrl or Alt + letter alone), with the same
  warning toast as typing keys: a lost Ctrl+D is worse than binding it by hand in the file.
  Ctrl+digit/symbol, Ctrl+Shift/Ctrl+Alt + letter, Super and F-keys stay allowed. [user]
- **Tab keys with no tab open do nothing** (Ctrl+Shift+W, Ctrl+Tab, Ctrl+1..9, next-attention,
  duplicate): a key that closes tabs must never close the window (unlike VS Code's Ctrl+W).
  No beep or toast. [UX agent]
- **The tab list takes keyboard focus as one list**, entered with Down (or Tab) from the
  "Search tabs..." field; the focused row is always the selected tab (Up/Down/Home/End select
  visible rows, like Ctrl+Tab), marked by a focus ring on top of the selection color. Enter,
  Escape or Tab hands the keyboard to the terminal and ends the search like the field's own
  Enter/Escape (one rule: leaving a search for the terminal clears it, so no tab stays hidden
  by a forgotten filter); Shift+Tab goes back to the field, keeping the query. So Tab and
  Shift+Tab walk the usual focus order field, list, terminal (the explorer takes no keyboard
  yet). A match inside a collapsed group expands it for good when chosen, as the field's
  Up/Down does. While the Settings page shows, entering the list moves the keyboard only (the
  ring sits on the selected Settings row, not a list entry); the first Up/Down selects the
  tab entering would have, Home/End the first/last, and leaving goes back to the page, so
  entering never swaps the page away by itself. A mouse click on a row still focuses the
  terminal as today. Rejected: F2 "anywhere but the terminal" (it would fire from the file
  explorer, where F2 should one day rename files, and still needs a hidden target). Built
  before F2, as its own step. [UX agent; split confirmed by user]
- **F2 is the default key of a rebindable "Rename tab" action** bound only while the tab
  list has focus, so the terminal never loses F2. The tab menu shows its current key like
  Duplicate does; a rename started from the list gives the keyboard back to the list.
  [UX agent]

## Opacity and blur

### Opacity: what turns translucent below 100 %?
The terminal's default background and padding, sidebar and file explorer, including their
selected and hovered rows, share the opacity setting. Cell backgrounds, selection, cursor,
file viewer, Settings, title bar and overlays stay opaque. At 100 % the appearance is unchanged.
Rejected: whole-window glass (contrast loss), translucent cell colors (agent UIs unreadable).
[user: extend opacity to the sidebar and explorer]

- **Slider**: 30-100 %, step 5, integer percent in the file, live while dragged like font
  size; no preview on the Settings page. [UX agent]
- **No special cases**: light theme, full screen and maximized keep opacity; X11 without a
  compositor isn't detected (unreliable), it's a known limitation. [UX agent]
- **Linux client-decoration shadow** darkens the see-through area by about a third: accepted
  and documented; the real fix is upstream in gpui-component (posting is the user's call).
  [UX agent]

### Blur
- One file value `none | blur | mica` with platform labels (Windows: None/Acrylic/Mica; Linux:
  None/Blur). Mica falls back to Acrylic before Windows 11 22H2, with a caption. On Linux it's
  available on Wayland + KDE only (`XDG_CURRENT_DESKTOP`); otherwise the dropdown is disabled
  with "Not available on this system". Mica Alt left out. [UX agent]
- At 100 % opacity the dropdown is disabled with "Needs Opacity below 100 %" (the dependent-
  setting rule); "Not available" wins when both apply. Its "Reset to default" still shows, as
  on other dependent rows. [UX agent]
- Switching live on Windows must apply live: the app clears the previous backdrop itself
  (`window_background.rs`). Mica extends the DWM frame across the client area; leaving it
  resets that extension. A failed DWM call shows a persistent warning with a restart action
  in its text. Only add "Mica stays until the app restarts" as a caption and known limitation
  if live Windows checks establish that behavior. [UX agent]

## Sidebar and tab status

- **Working spinner** (Copilot; Claude Code only while a custom name hides its animated
  title glyph, so never two animations): a rotating Lucide `LoaderCircle` in the attention
  triangle's slot, Working color, row only (cards, drag preview and collapsed groups stay
  static); the title shifts like it does for the triangle. With "Activity animations" off it
  is the same icon, still. One shared `SpinnerClock` (`workspace_view/spinner_clock.rs`)
  steps it, and only while a turning spinner is drawn. [user: redraw rate and shared clock]
- **"Status unavailable"** (agent hooks failed): Claude tabs only, replacing "No recent
  output"; "Recent output" stays. Looks and counts like Quiet (no badge, not in attention or
  group chips). Copilot tabs are unchanged (their status comes from the CLI). [UX agent]
- **In-place agent in a shell tab** (agent mark until its end mark, which the typed `claude` /
  `copilot` functions print when the agent exits, else the command's `D`): while it runs the
  tab looks and notifies exactly like a `+` menu agent tab (agent icon, status chip with lap
  time, Mode line, attention, toasts, bell ignored for Claude, idle agent closes without a
  question, no teal Running badge or "Running for" line); the card's kind line names both
  ("Claude Code in bash") since restore starts the shell. The name stays title-derived; at
  the run's end the title from before the run comes back (agents may leave theirs, glyph
  included) and the shell's own title, if it sets one, wins as usual. [UX agent]
- **When the in-place run ends** (the end mark, else `D`, or the shell itself exiting) in a
  background tab, the agent-tab exit toast fires with the same rule (code when unsuccessful,
  none after a clean exit with a Ready for review pending); nothing while selected. A pending
  Ready for review is kept until the tab is selected (the unread turn is still on screen),
  while Needs attention from the agent (Waiting, bell) is cleared: nothing waits any more, the
  toast says why. The shell exiting mid-run ends as a plain Exited shell tab (shell icon, hollow
  ring), no other special state. Rejected: shell-style silence, which loses a crashed
  agent in a background tab. A second agent mark in the same command switches to the new
  agent silently, as a new turn would (no exit code exists between marks, the run's end
  still toasts at `D`); the title from before the first agent is the one put back. With the
  end mark, the rest of the line (`claude; make`) is a shell command again: teal Running once
  it has run for the usual delay, timed from the end mark, and the later `D` neither toasts
  nor changes anything else: the agent's time is already in its status lap, and counting
  it would show a long-running `make` the instant the agent exits. A leftover Ready for
  review outranks later shell activity (Running, Recent output) until the tab is selected, as
  on an agent tab; a bell still outranks it. [UX agent]
- **The user's own `claude` / `copilot` alias** is wrapped when its expansion starts with the
  agent's own name (or `command` + name): that first word becomes the in-place function and
  the rest is kept as written, so the common `alias claude='claude --some-flag'` gets the
  status instead of silently losing it. Any other alias, and every user function, is kept
  untouched (README says how to add the mark by hand). Rejected: rewriting arbitrary
  expansions or function bodies (fragile), and a warning (scripts can't show one cheaply,
  and wrapping fixes the common case). [UX agent]
- **Hover card under the cursor when a hold ends** (rename, menu, dialog, drag) stays hidden
  until the pointer leaves the row, like Windows/VS Code tooltips. [UX agent]

## Toasts

- **Attention toast**: the jump key is in a tooltip only, worded "Go to the next tab needing
  attention (keys)" like the sidebar button. Its text isn't updated when the wait reason
  changes (a toast is a nudge; the sidebar is the live state). [UX agent]
- **Agent-status-off warning**: "Agent status is off: " + cause + fix per failure (missing
  hook program, listener, settings write, other). Shown once per window start, even with
  notifications off or only shell tabs, fixed id. Stacked above the fallback-shell warning,
  not merged. [UX agent]

## New tabs and the empty state

- **Placement**: every in-window new tab (`+`, Ctrl+Shift+T, explorer entries, duplicate) opens
  right below the selected tab, in its group; below a pinned tab it goes to the first unpinned
  position (never pins it). The command-line tab stays at the end of the top level. From the
  settings page, a collapsed group or a search-hidden tab: same rule, `activate` reveals it.
  [UX agent]
- **Closing the last tab keeps the window** with an empty state: the `+` menu's launcher list
  as buttons (default shell first and filled), a Ctrl+Shift+T hint, keyboard focus on the
  default button. A tab opened from it takes the last closed tab's folder, at the end of the
  top level. No special prompt on any close path; groups stay. [user: keep the window]
- **With no tab**: the explorer is hidden (state remembered), "Open settings file" is disabled
  ("Open a tab to view the file"), search and groups work as usual. [UX agent]
- **Next start with zero saved tabs** restores the groups and opens the default shell as
  usual, so there is always a tab after launch. [UX agent]

## Settings page and file panes

- **Opening a file from the page**: leaving the page focuses the tab's terminal like any
  other exit; Cancel on "Discard changes?" leaves the user on the edited file, not back on the
  page. Whether opening a file should focus the viewer is open for all open paths at once
  (needs the user). [UX agent]
- **The page hides the explorer**: the explorer button and Ctrl+Shift+| read it as hidden;
  pressing them leaves the page and shows the explorer. Every explorer-on path (viewer folder
  links too) goes through that helper. [UX agent]
- **Explorer dimming follows in-place `.gitignore` edits** (watcher lets `Modify(Data)` through
  for `.gitignore` only). `.git/info/exclude`, global excludes and tracking changes still need
  a collapse/expand. [UX agent]

## Command history popup (per shell)

- **Chips**: "All" first, then shells in new-tab menu order, then unknown keys by label; icon +
  menu name, unselected ones truncate. Alt + underlined first letter switches (again cycles
  same-letter chips; Ctrl+Alt never, for AltGr); click works too. Chips are hidden when only
  the tab's own shell has entries. [UX agent]
- **Opens on the tab's own shell**, even when empty ("No <label> commands yet · Alt+A: all
  shells"), so the state depends on the tab, not the data. [UX agent]
- **Shell key** of a tab: a discovered launcher id, else derived from program + args (WSL
  distro, Git Bash, file stem), computed once at session start. [UX agent]
- **Delete under "All"** forgets the command in every shell; the footer says so. [UX agent]

## Themes and terminal colors

### Light theme too white
One softened Light palette (warm grey, no white surface, darker muted text, visible borders,
theme-specific status colors) instead of a variants picker or user theme files; the Appearance
switch became a Theme dropdown. Built as a data table so variants can follow. Values and
contrast targets: `light_theme.rs` (state is never color-only, so Quiet stays low on purpose).
[UX agent; variants: user]

### Sepia theme
A third Theme choice, built like Light (`light_theme.rs`, `SEPIA`). [user]

### Terminal colors that follow the theme
A "Terminal colors" dropdown (Match theme / Dark / Light / Sepia) [user]. Light and Sepia
palettes map white to greys like VS Code Light, so white text stays readable (rejected:
Solarized-style light white, which vanishes); the background is the theme's raised surface,
never white. Dim blends toward the background on light palettes; Sepia gets its own search
colors (the default ones clash with its selection). Values: `term-core/src/palette.rs`. New
tabs get `COLORFGBG`; it goes stale if the setting changes while a tab runs (known
limitation). [UX agent]
