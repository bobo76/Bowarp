# UX/UI review

Two reviews merged and re-checked against the code on 2026-11-12 (0.3.0, 9faedce, branch
`git-page`). Items that are built have been removed; duplicates are merged into one entry.

- **2026-10-25 review** (0.2.30, validated 2026-11-01 at 0.2.56): the whole app against
  README.md, FEATURES.md, BACKLOG.md, docs/known-limitations.md, docs/ux-decisions.md and the
  Warp references in `docs/ui-reference/`, with live screenshots under Xvfb (1200x800, then
  640x480), cited as "shot NN" and not added to the repo. Not seen live: title bar window
  controls, maximize, opacity on a real desktop, the light theme, Windows. Its items keep their
  numbers (`#2`...`#34`) because BACKLOG.md refers to them.
- **2026-11-11 review** (0.2.61): what a user meets, read from the docs and the code; **the app
  was not run**, so it makes no visual claims. Its items are `F` (fix), `E` (enhance) and `M`
  (missing).

Items already planned in BACKLOG.md (command palette, viewer close shortcut and search, new-tab
folder picker, async explorer reads, TerminalView caching, extended group card, Git page
follow-ups) are not repeated.

Removed as built: #1, #9, #13, #14, #21, #22 (as E4: resume by session id), #24, `RenameTab`
and the previous-tab part of #30 (`LastTab`, Ctrl+`), F1 and F9 (save or discard unsaved
edits, also at window close), F2 and F3 (tabs saved as arranged; guest-window notice), E4,
E11 (Back / Forward / Last tab), E5 (changed files colored in the explorer and listed under
the card's `+N`; its step 3, a diff panel, is now the Git page's diff view). M2 is merged into
#11.

## Verdict (2026-11-11)

The core promise works and is well thought through: per-tab agent status from the agents' own
hooks, attention jump, groups, hover cards, in-place agents in shell tabs, safeguards that
can't be switched off, settings that survive hand edits. The weak spots, by cost to a user:

1. **Losing work silently**: the agent's last screen (F4), permanent deletes (F5), remote
   sessions ended on close (F7). (Unsaved edits and tab layout are fixed.)
2. **The attention promise stops at the window edge**: nothing reaches a user in another
   application (#11), and remote tabs have no status (F8, M3).
3. **Baseline terminal comforts are missing**: no right-click menu, no paste protection, no
   scrollback control, no jump between prompts (E1-E3, F6).
4. **Nothing explains the app from inside it** (E6, F10).
5. **The spec says "side by side" and "panes"**, but one terminal shows at a time (M1).

## Summary

Impact and cost: S under a day, M a few days, L more. "User" = needs the user's decision.
Status: **Valid** (confirmed in the code on 2026-11-12), **Unverified** (needs a live check),
**Design** (a judgment, no factual claim).

| ID | Area | Suggestion | Impact | Cost | User | Status |
|---|---|---|---|---|---|---|
| #2 | Layout | Viewer width from the free space; terminal keeps ~60 columns | High | S | no | Valid |
| #3 | Layout | Resizable sidebar, saved like the explorer width | Medium | M | no | Valid |
| M1 | Layout | Split panes: build a minimal one or take them out of the spec | High | L | yes | Valid |
| #4 | Sidebar | Name shell tabs by command / folder, not `user@host: path` | High | S | yes | Valid |
| #5 | Sidebar | Group rail so group membership is visible | Medium | S | no | Design |
| #6 | Sidebar | Tell a failed exit from a clean one on the row | Medium | S | no | Valid |
| #7 | Sidebar | Compact row density for many tabs | Medium | M | yes | Design |
| #8 | Sidebar | Label the hover card's `+N` | Low | S | no | Valid |
| #10 | Sidebar | Prompt redraws shouldn't count as "Recent output" | Low | S | no | Unverified |
| #11 | Attention | Taskbar flash, then OS notification and sound when unfocused | High | S-M | yes | Valid |
| #12 | Attention | Window title follows the selected tab and attention count | Medium | S | no | Valid |
| #15 | Errors | Start failures open the window with an error, not exit | High | S | no | Valid |
| #16 | Errors | Say when a folder can't be watched | Low | S | no | Valid |
| F4 | Agents | Keep the agent's last screen when its tab goes on as a shell | High | M | yes | Valid |
| F5 | Files | Explorer Delete moves to the system trash | Medium | S-M | no | Valid |
| F6 | Terminal | Warn before a multi-line paste the program can't bracket | Medium | S | no | Valid |
| #17 | Terminal | Restart an exited tab (button + Enter) | Medium | S | no | Valid |
| #18 | Terminal | Close a shell tab on a clean `exit` | Medium | S | yes | Valid |
| #19 | Terminal | One-time "Hold Shift to select" hint under mouse reporting | Low | S | no | Valid |
| E1 | Terminal | Right-click menu: Copy, Paste, Select all, Search, Clear | Medium | S-M | no | Valid |
| E2 | Terminal | Scrollback size setting, Clear scrollback, scroll to top/bottom | Medium | S | no | Valid |
| E3 | Terminal | Jump to previous / next prompt; copy a command's output | Medium | S-M | no | Valid |
| #20 | Tabs | Reopen closed tab | Medium | M | yes | Valid |
| E7 | Launch | Custom entries in the `+` menu (command, folder, arguments, name) | Medium | M | no | Valid |
| F7 | Remote | Closing a remote tab or the app: Detach or End, not always End | High | M | yes | Valid |
| F8 | Remote | Say on the tab that status isn't available for remote tabs | Medium | S | no | Valid |
| M3 | Remote | Agent status for remote tabs, before the operator tools | High | L | no | Valid |
| #23 | Explorer | Keyboard navigation in the file tree | Medium | M | no | Valid |
| M5 | Files | Quick open (fuzzy file name search) in the command palette | Medium | M | no | Valid |
| #25 | Settings | Narrow the settings nav, keep inputs inside the row | Medium | S | no | Valid |
| #26 | Settings | Shorten row descriptions to one sentence | Medium | M | no | Design |
| #27 | Settings | Say when the font family isn't installed | Medium | S | no | Valid |
| #28 | Settings | Wording: "Bowarp", Tab vs last-session text, Keyboard subtitles | Low | S | no | Valid |
| E8 | Appearance | UI text size; follow the OS theme; cursor shape and blink | Medium | S-M | no | Valid |
| E9 | Settings | Apply `settings.json` edits when the file changes | Low | S | no | Valid |
| E10 | Copilot | Install the mode extension from Settings | Low | S | no | Valid |
| #29 | Shortcuts | All shortcut hints in menus/tooltips; "Copy title" | Low | S | no | Valid |
| #30 | Shortcuts | Actions for pin and move tab | Medium | S | no | Valid |
| #31 | Shortcuts | Ctrl+9 selects the last tab | Low | S | yes | Valid |
| #32 | Title bar | Drop the `i` About button (Settings -> About stays) | Low | S | yes | Valid |
| E6 | Onboarding | First-run tips and a status legend inside the app | Medium | S | no | Valid |
| F10 | Docs | Split the README; fix contradictions | Medium | S-M | no | Valid |
| M4 | Updates | "Check for updates" on the About page | Medium | S-M | yes | Valid |
| #33 | A11y | Give Running a shape, not only a teal color | Low | S | no | Valid |
| #34 | A11y | A left accent bar on the selected row | Low | S | no | Design |

## Layout, dividers and narrow windows

### #2. Viewer width from the space right of the fixed panes; a real terminal minimum
- Problem: `viewer_width` is still `fraction * row` of the whole window, only capped at
  `row - fixed - MIN_TERMINAL_WIDTH` (`pane_width.rs:19-21`). With the sidebar (240) and
  explorer (300) a 45 % viewer leaves the terminal its 240 px minimum (`pane_width.rs:14`):
  ~28 columns, the prompt wraps (shot 07). The terminal is the main thing and loses first.
- Option A: fraction of `row - fixed`, terminal minimum raised to ~60 columns (cell width x
  60, or 480 px). Pros: the terminal stays usable; the saved fraction keeps its meaning. Cons:
  the viewer is narrower with the explorer open.
- Option B: auto-hide the explorer when opening a file would push the terminal under the
  minimum. Cons: a pane vanishing on a click surprises.
- Recommendation: A, a pure-function change in `workspace-model`, unit-testable.
- Edge cases: in a window too narrow for everything the viewer minimum (240) still wins, as
  today; saved `file_viewer_fraction` values keep loading (meaning shifts slightly).

### #3. Resizable sidebar
- Problem: the sidebar is a fixed 240 px (`SIDEBAR_WIDTH`, `sidebar.rs:31`) while the explorer
  and viewer can be dragged. Titles truncate after ~16 characters (shot 03); at 640 px it
  takes 37 % of the window (shot 13).
- Recommendation: the explorer's 6 px edge strip, 180-400 px, double-click resets to 240,
  saved as `sidebar_width` with the explorer width's rules. Reuse the explorer divider code.
- Edge cases: hover cards and the drag preview use `SIDEBAR_WIDTH` and must follow the live
  width; the attention button and `+` must stay visible at 180 px.

### M1. Split panes (needs the user)
- FEATURES.md:32 says "Support split panes, adjustable pane sizes" and :80 "tabs and panes";
  the README's first line says "side by side". One terminal shows at a time and BACKLOG.md has
  no pane item.
- Why it matters here: the typical use is watching one agent while working in a shell on the
  same folder, or comparing two agents. Badges reduce the need but don't replace the output.
- Smallest useful version: one vertical split of the terminal area showing two existing tabs
  (the selected one plus one "kept visible" from the tab menu), no nested layouts. The sidebar
  stays the single list of sessions.
- Decision: build that, or remove panes from FEATURES and the README wording.

## Sidebar and tab rows

### #4. Shell tab names that tell tabs apart (needs the user)
- Problem: a shell row shows the program's title, `user@host: ~/path` for bash, truncated to
  "bobo@boboBee…"; two shells in different folders look identical (shot 03), while the second
  line already shows the branch / folder.
- Option A: with shell integration, the running command while one runs, else the folder's last
  component; the program title moves to the hover card ("Program title" exists). Pros: rows
  differ and say what happens; uses tracked OSC 133 data. Cons: shells without the integration
  still show titles.
- Option B: strip a leading `user@host:`. Cons: heuristic, prompt formats vary.
- Recommendation: A; it changes what every shell row says, so it needs the user.
- Edge cases: a custom name always wins; agent tabs keep their titles; `ssh` shows "ssh host".

### #5. Group rail
- Problem: grouped rows differ from ungrouped ones only by a 12 px indent; the ungrouped row
  right after a group looks like part of it (shot 16).
- Recommendation: a 2 px vertical line from the group header to its last row, in the group's
  color (muted border when none), at the chevron's x. Collapsed groups draw nothing; search
  shows it on matched rows only; the drop marker stays visible over it.

### #6. Failed exit vs clean exit
- Problem: Exited is a hollow ring in `#545d68`, the same grey as Quiet, Unavailable and
  Disconnected (`sidebar.rs:122`), whatever the exit code. A failure reads like a clean end.
- Recommendation: nonzero or unknown code -> a red ring with a small `×` and "Exited (3)" in
  the card; code 0 keeps the grey ring, raised to >= 3:1 contrast.

### #7. Compact rows for many tabs (needs the user)
- Problem: rows are 57 px, ~12 tabs fit at 800 px; Ctrl+1..9 reaches only 9.
- Recommendation: a "Row density: Comfortable / Compact" picker on Appearance; compact is one
  32 px line, branch/folder in the card. Needs the user: it contradicts FEATURES "git branch
  ... under its name".

### #8. Label `+N` in the hover card
- Problem: the card shows a bare green "+5" (`sidebar.rs:994`, `:1113`): lines? files? It now
  also opens the list of changed files.
- Recommendation: "+5 lines", with a tooltip "Lines added in the working tree".

### #10. "Recent output" for a shell at its prompt (unverified)
- Problem: a selected bash tab at its prompt shows "Recent output" (shot 13): the prompt
  redraw counts as output.
- Recommendation: in integrated shells, output between the `D`/`A` and `B` marks doesn't
  refresh Active. Low priority.

## Status, attention and notifications

### #11. Reach the user outside the window (needs the user; was also M2)
- Problem: the app's main promise is "know when an agent needs you", but it only works while
  the user looks at the window; users leave agents running precisely to do something else.
  GPUI has `Window::request_attention` (Windows taskbar flash, X11 urgency, nothing on
  Wayland) and `App::show_system_notification`; neither is used.
- Recommendation, in order: (1) `request_attention()` when a tab turns Needs attention or
  Ready for review while the window isn't active, following "In-app notifications", cleared on
  activation (designed in BACKLOG.md); (2) OS notifications behind their own switch (they
  work on Wayland); (3) sound (parked on an ALSA build dependency). Treat this as the top
  feature gap.
- Decision: the go-ahead for (1); defaults for (2) and (3).

### #12. Window title follows the selected tab
- Problem: the OS title is always "Agent Workspace" (`workspace_view.rs:430`); two instances
  look the same in Alt+Tab and the taskbar.
- Recommendation: "<tab name> — Agent Workspace", prefixed "(2) " while tabs need attention.
  Set on selection, rename and status change only. Settings shown -> "Settings — Agent
  Workspace".

## Error states (Windows release builds have no stderr)

### #15. Command-line and first-tab failures open a window
- Problem: bad arguments (`main.rs:62`), a folder that isn't a directory (`main.rs:130`) and
  a shell plus fallback that both fail (`main.rs:351`) print to stderr and exit. On Windows a
  pinned shortcut to a removed folder looks like "the app doesn't start".
- Recommendation: a missing folder opens the tab in the home folder with an error toast; a
  total start failure opens the window with an error panel (message, "Open settings", Retry);
  bad arguments show the usage in the same panel. Keep the stderr lines for Linux.

### #16. Say when a folder can't be watched
- Problem: a watcher error only logs (`folder_watcher.rs:63`); the tree and viewer then
  silently stop following disk changes (network mounts, inotify limits).
- Recommendation: a muted "Not following changes" caption with a refresh button in the
  explorer / viewer header. No polling.

## Agents and data loss

### F4. The agent's last screen disappears when it quits (needs the user)
- Problem: when Claude Code or Copilot CLI exits with code 0 the tab starts a shell and "the
  agent's output is not kept" (README, Design decisions). That screen holds the final summary,
  the cost line and the `claude --resume <id>` hint.
- Suggestion: keep the launch design and start the fallback shell below the agent's last
  output, so it scrolls into scrollback; if the terminal can't be reused across two processes,
  print the last screen into the new session before the first prompt.
- Decision: it changes a design decision recorded in the README.

### F5. Delete in the explorer is permanent
- Problem: Delete removes for good (`file_explorer.rs:1071-1077`), one row away from Rename.
  The dialog says so (`:611`), but every file manager uses the trash, so it reads as routine.
- Suggestion: move to the system trash (Recycle Bin, freedesktop trash); keep the permanent
  delete with today's wording where no trash exists (network mounts, WSL paths). A new
  dependency needs a `cargo deny` license check.

## Terminal view

### F6. A multi-line paste runs at once without bracketed paste
- Problem: outside bracketed-paste mode every newline becomes Enter (`keys.rs:66-68`). In cmd,
  a nested shell or plain ssh, a snippet with a trailing newline runs at once. Windows Terminal
  and iTerm2 ask first.
- Suggestion: without bracketed paste and with a newline in the text, ask once ("Paste 4
  lines? They will run as commands") with a preview. A safeguard, so no switch.

### #17. Restart an exited tab
- Problem: the exit bar offers only "Close the tab"; getting the same tab back (name, color,
  group, folder) means a new tab plus re-arranging. The block cursor stays on the dead grid.
- Recommendation: a "Restart" button and Enter (tab focused) restart the same launch spec in
  the tab's last folder, like Windows Terminal; `replace_tab_session` already restarts a tab in
  place. Hide the cursor once the process is gone. Scrollback is cleared (say so in the
  tooltip); agents restart with their saved args.

### #18. Close a shell tab on a clean `exit` (needs the user)
- Problem: `exit` leaves a dead tab; most terminals close it on a clean exit (Windows Terminal
  `closeOnExit: graceful`).
- Recommendation: shell tab, code 0 -> close; nonzero -> keep the bar (#17). Changes README
  "When the program exits, a bar...".

### #19. "Hold Shift to select" hint
- Problem: in Claude Code (mouse reporting) a drag selects nothing; the Shift override is only
  in the README. Likely the first thing a new user reports.
- Recommendation: on the first drag in a mouse-reporting tab per run, an info toast "Hold Shift
  to select text". One flag.

### E1. A right-click menu in the terminal
- Today the right button only reaches programs that track the mouse (`terminal_view.rs:1422`).
  Copy, paste and search are shortcuts only, which Windows users won't guess (Ctrl+Shift+C).
- Suggestion: Copy (disabled without a selection), Paste, Select all, Search, Clear scrollback;
  on a link: Open, Copy link. With mouse reporting, Shift+right-click. Each entry shows its
  shortcut.

### E2. Scrollback control
- The scrollback is alacritty's default size with no setting; search can't reach evicted
  output. No Clear scrollback action and no key for top/bottom (only Shift+PageUp/PageDown).
- Suggestion: a "Scrollback lines" setting (say what a line costs), a `ClearScrollback`
  action, Shift+Home / Shift+End.

### E3. Use the prompt marks for navigation
- OSC 133 marks are used only for status and history.
- Suggestion: Ctrl+Shift+Up / Down to the previous / next prompt (Windows Terminal, VS Code,
  kitty); "Copy output of last command" in the E1 menu: the common hand-off from a shell tab to
  an agent tab.

## Tabs and launching

### #20. Reopen closed tab (needs the user)
- Problem: a closed tab loses its name, color, group, pin and folder; the Warp reference has
  "Reopen closed session".
- Recommendation: keep the last 10 closed tabs' `tabs.json` entries in memory; "Reopen closed
  tab" at the bottom of the `+` menu and an unbound `ReopenClosedTab` action. Starts a new
  session in that folder and place.

### E7. Custom entries in the `+` menu
- `claude_args` / `copilot_args` are global, so "Claude in plan mode" and "Claude with
  skip-permissions" can't both be one click away; a dev server, REPL or plain `ssh` needs a
  shell tab plus typing.
- Suggestion: a `launchers` list in `settings.json` (name, base launcher or command,
  arguments, optional folder and icon), shown after the built-in entries and saved in
  `tabs.json` by id. Absorbs the backlog's "Configurable terminal icon".

## Remote

### F7. Remote sessions end when the tab or the app closes (needs the user)
- Problem: docs/remote.md promises sessions that keep running "like tmux", but "Closing the app
  ends its remote sessions, like local ones" (`remote.md:158`). Shutting the lid may keep an
  agent running; quitting the app properly kills it.
- Suggestion: closing a remote tab (or the window with remote tabs) asks "End the session on
  <host>" or "Leave it running"; sessions left running are listed in the host's `+` submenu.
  Ship it with the backlog's remote Restore item.

### F8. Remote tabs look like local ones but report nothing
- Problem: remote tabs have no agent status, branch, history or explorer; a remote Claude
  waiting for permission looks idle.
- Suggestion until M3: the hover card's status reads "Status isn't available for remote tabs
  yet" (the explorer message is already in BACKLOG.md). Check that a bell from the remote
  program still raises Needs attention.

### M3. Agent status for remote tabs
- Tracked under Remote sessions after several server-side items. It should come first: a
  remote agent without status is a plain ssh window; client counts and `show` are operator
  tools.

## File explorer

### #23. Keyboard navigation in the tree
- Problem: `file_explorer.rs` has no focus handle; every action is mouse-only, against
  "keyboard first". The only pane a keyboard user can't use.
- Recommendation: Up/Down move, Right/Left expand/collapse, Enter opens, F2 rename, Delete
  asks, Shift+F10 / Menu key opens the entry menu; a visible focus row; opening the explorer by
  its shortcut focuses it, Esc returns to the terminal.

### M5. Quick open
- Opening a known file means expanding folders by hand.
- Suggestion: a file mode in the planned command palette (fuzzy match under the tab's folder,
  ignoring git-ignored entries), read off the UI thread.

## Settings and appearance

### #25. Narrower settings nav; inputs inside the row
- Problem: the nav column is up to 360 px next to the 240 px app sidebar; the Font family
  input runs past the right edge at 1200 px (shot 09). `settings_page.rs` sets no width.
- Recommendation: `SettingsView::sidebar_width(px(200.))` and a max width on text inputs.

### #26. One-sentence descriptions
- Problem: rows carry README paragraphs ("Command history": 7 lines, shot 11).
- Recommendation: one sentence per row (what it does + what off means); details in a tooltip
  or the README. Keep the disabled-reason captions.

### #27. Font not installed
- Problem: a missing family silently falls back (known-limitations "Terminal font").
- Recommendation: a caption "Not installed; using DejaVu Sans Mono"; later a dropdown of
  installed monospace fonts.

### #28. Wording fixes
- "Bowarp's history" (`settings_page.rs:536`, `:572`): the UI says Agent Workspace elsewhere.
- "accepted with Right, End or Tab, including the last session's command"
  (`settings_page.rs:561`): Tab doesn't accept the restored last-session command.
- Keyboard page: '"Copy" in settings.json.' under every action (`keyboard.rs:92`) is noise;
  move the action id to a tooltip.

### E8. Appearance gaps
- UI text size: only the terminal font is adjustable; one "Interface size" setting (rem size)
  helps HiDPI and low-vision users.
- Follow the OS theme: README:614 "Following the OS theme is not included". Add "System".
- Cursor shape (block, bar, underline) and blink; blink off by default, tied to "Activity
  animations".
- Reduced motion: when the OS asks for it, start with Activity animations off.

### E9. Apply `settings.json` edits when the file changes
- Hand edits apply only when the settings page opens or the file is saved in the viewer
  (known-limitations:135). Watching one file is event-driven; it matters most for
  keybindings, partly file-only.

### E10. Install the Copilot mode extension from Settings
- Today two files are copied by hand into `~/.copilot/extensions/bowarp` (README:322-328) and
  again after every update. Suggestion: a row on Settings → Terminal ("Not installed /
  Installed / Older version", Install / Remove), on an explicit click only.

## Shortcuts, menus and discoverability

### #29. Shortcut hints everywhere they apply
- Tab menu: "Duplicate tab" shows its key, "Close tab" doesn't show Ctrl+Shift+W
  (`tab_menu.rs:124`).
- "Copy pane title" (`tab_menu.rs:102`): the app has no panes; "Copy title".
- "Close explorer" tooltip lacks its key (`file_explorer.rs:866`).
- "Search tabs..." placeholder: the key in a tooltip.
- All through `shortcuts::with_shortcut` / `menu_label_with_hint`, so rebinding stays right.

### #30. Actions for pin and move tab
- Problem: pin and move up/down are tab-menu commands only (`TabCommand::TogglePin`), so the
  Keyboard page can't bind them and the palette will have nothing to call.
- Recommendation: `TogglePin`, `MoveTabUp`, `MoveTabDown` actions; Ctrl+Shift+PageUp /
  PageDown for move, pin unbound. Moves across group boundaries behave like the menu's.

### #31. Ctrl+9 = last tab (needs the user)
- Chrome, VS Code and Windows Terminal map Ctrl+9 to the last tab; here it is the 9th
  (`workspace_view.rs:157`). Changes the README.

### #32. Drop the title bar About button (needs the user)
- The `i` button (`about.rs:31`) takes a title bar slot for a rare action; Settings -> About
  already has the content.

### E6. Explain the app from inside the app
- A first start shows one shell tab. Nothing says `+` holds agents, what the badges mean, that
  Ctrl+Shift+A jumps to the tab that needs you, or that `claude` typed in a shell tab gets
  status. The eight badge states are defined only in a README table.
- Suggestion: a one-time "Getting started" card (three lines), first window only; a status
  legend (About page or a `?` by the attention button); disabled agents in the `+` menu say
  how to install them; a shortcut or tooltip pointing at the Keyboard settings page.

### F10. Documentation is written for the builder, not the user
- README.md is 892 lines and mixes the user guide with Copilot payload details, verification
  notes, manual checklists and release mechanics; no screenshot.
- Suggestion: README = what it is, a screenshot, install, a two-minute quick start, the key
  shortcuts, links; the rest in `docs/user-guide.md` (by task) and `docs/`.
- Contradictions found (still present):
  - Product name: "Agent Workspace" in the UI, "Bowarp" in README:290, the Copilot extension's
    folder and #28; `agentws` in file names. Pick one user-facing name.
  - README:513 says the viewer width "is not saved yet"; known-limitations:155 says it is
    (`file_viewer_fraction`).
  - README:115 and :144 say a click opens a link; FEATURES.md:28 says Ctrl+click.
  - FEATURES.md:29 ends with a stray "www.google.ca".
  - README:236 says the folder follows `cd` on Linux; the explorer section says bash, zsh and
    PowerShell tabs follow on other platforms too.

### M4. Update check (needs the user)
- Installs are manual and nothing says a newer version exists, while each feature ships as a
  patch release.
- Suggestion: "Check for updates" on the About page, on click only (one request to the
  releases feed), showing the version and a link to its notes. An automatic check, if wanted,
  is a switch that starts off.

## Accessibility and theming

### #33. Running needs a shape
- Working (blue) and Running (teal) are both 12 px dots; Running pulses only for 10 s, and
  Activity animations off removes even that.
- Recommendation: a small inner dot or ring on the Running badge.

### #34. Selected row accent
- The selected row is only a slightly lighter background (shots 03, 16).
- Recommendation: a 2 px accent bar at the row's left edge, in the theme's accent color.

## Suggested order

BACKLOG.md holds the top 10 (#11, #15, #17, #2, F6, #6, E1, #4, F5, #12, in that order) and
Tier 2.

1. Tier 1: #15 start failures, #2 viewer width, #11 step 1 taskbar flash, #17 restart, #12
   window title, #6 failed exit.
2. Tier 2, one batch: #29 + #28 + #8 wording and hints, #25 settings layout, #19 Shift hint.
3. Remaining small safety fixes: F5 trash, F6 paste warning.
4. Terminal comforts: E1, E2, E3.
5. Attention beyond the flash: #11 steps 2-3.
6. Onboarding and docs: E6, F10 (its contradictions are minutes of work).
7. Remote, as one theme: F8 now; F7 with the backlog's Restore; M3 before the operator tools.
8. Worth doing: #4 shell tab names, #30 actions, #5 / #33 / #34 visuals, #16, F4.
9. Later (M+ cost or a product decision): M1, #3, #23, M5 (with the palette), #20, E7, E8,
   E9, E10, M4, #7, #26, #27, #18, #31, #32, #10.

## Decisions for the user

#4, #7, #11 (go-ahead, then notification and sound defaults), #18, #20, #31, #32, F4, F7, M1,
M4.
