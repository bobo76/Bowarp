# Agent Workspace - Backlog

Backlog and next steps. The target is [FEATURES.md](FEATURES.md); how built features behave at
their edges is in [docs/known-limitations.md](docs/known-limitations.md); finished work is in
the git history and [CHANGELOG.md](CHANGELOG.md). Remove an item once it is built and verified;
an item only missing its manual check moves to "Fixed, not tested yet", which lists the checks
still to do (Windows included), not what was already tested.

## Fixed, not tested yet

Built with automated coverage on Linux; the manual checks below are still to do.

Multi-line paste warning:

- Windows: pasting two lines into a cmd tab and into a PowerShell tab asks "Paste 2 lines?";
  Cancel sends nothing, Paste runs both. In a WSL bash tab (bracketed paste on) it doesn't ask.
- Plain `ssh` to a host whose shell has bracketed paste on doesn't ask; in a nested `sh` it
  asks.

Viewer width from the free space:

- Windows, at 100% and at a 150% display scale: with the sidebar and explorer open, the
  viewer divider dragged to the far left leaves the terminal about 60 columns; in a window
  too narrow for both the viewer keeps 240 px.

Exited tabs (Restart, failed exit ring):

- Windows: a PowerShell, cmd and WSL tab that ran `exit 3` shows the red ring with the cross
  and "Exited (3)"; Restart, Enter and the tab menu start it again in its last folder.
- Light and Sepia themes: the grey Exited ring (code 0) and the red one read clearly on a
  selected and an unselected row.
- A Copilot tab whose CLI fails (nonzero exit) restarts with its saved arguments; a real
  Claude Code tab started with arguments (e.g. `--model`) restarts with them, no `--resume`.

Start failures open the window:

- Windows release build (no console): `agent-workspace bogus` opens the window with
  "Invalid command line" and the usage; a missing folder opens the tab in the home folder
  with the warning; a first tab that cannot start (e.g. Claude Code not installed,
  `agent-workspace claude`) shows "Could not start", Retry and Open settings.
- Windows: a default shell whose program cannot start, with the automatic shell (Git Bash /
  PowerShell) working, still opens the fallback with its warning.

Window attention and title:

- Windows: with another window active, a Claude tab asking for permission (selected or not)
  flashes the taskbar button once; activating the window stops it. Nothing flashes while the
  app is the active window.
- A real X11 desktop (GNOME/KDE on Xorg): the urgency mark (highlighted taskbar entry) appears
  and clears when the window is focused.
- The taskbar / Alt+Tab label follows the selected tab, its "(N)" count and the Settings / Git
  pages (Windows and a Linux desktop).

Git page, page and file staging:

- Two tabs in different repositories and one outside any: the picker lists the two, the
  selected tab's first. Stage and unstage a modified, a deleted and an untracked file, and
  Stage all / Unstage all; `git status` in a shell tab agrees.
- Edit a file in a shell tab while the page shows: the list updates (on a `git add` at once,
  on a plain edit at the latest on Refresh). Hide the page: the CPU is idle. On Windows, no
  console window flashes.

Git page, diff view:

- Modified, added, deleted, renamed (staged), untracked, binary and CRLF files show right in
  the dark and light themes.
- An agent editing the selected file updates the diff without losing the scroll; Up/Down
  through many files stays responsive; the CPU is idle with the page shown and nothing
  changing.

Git page, commit:

- Commit from the page with the button and with Ctrl+Enter, then check `git log` in a shell
  tab; Enter adds a line, Tab moves to the list, Esc goes to the list.
- A failing hook in a real project (e.g. a lint hook): its output shows, the message and the
  staged files are kept.
- A commit with signing turned on (note how it behaves on Windows and Linux), and one while an
  agent tab is running in the same repository.
- The message grows to six lines, then scrolls; each repository keeps its draft when switching
  in the picker.

Git page, line and hunk staging:

- Stage a single line, a range across two hunks (Shift+click, drag) and a hunk, then compare
  with `git diff --cached` in a shell tab; unstage them again from the Staged diff.
- A CRLF file, a file without a final newline, a deleted file (staging part of it keeps the
  file in the index) and a new staged file (unstaging part of it).
- With an agent editing and staging the file meanwhile: no wrong lines are staged, an error
  shows instead.
- Keys in the diff: Up/Down over changed lines only, Shift+Up/Down, `S`/`U`, Shift+`S`/`U`,
  Escape twice; the right-click menu and the selection bar.

- Tabs start at the terminal's size: on Windows, with several Git Bash tabs, close and
  restart the app, click each tab and press Right arrow on the restored command: it comes
  back whole (it lost its first letter about one time in four). `window.json` has a
  `terminal` entry; opening a new tab doesn't redraw its prompt (no resize).

- Tabs saved as they change: rename, pin, color, group and move tabs, then kill the app
  (`kill -9`) and start it again: the arrangement is back. A second window shows "Tabs in
  this window aren't saved..." until closed; none with Restore previous tabs off.

- Resume on restore: a Claude Code tab with a conversation, closed and reopened, shows the bar;
  Resume it shows the old conversation (`--resume <id>`), Start new and typing a prompt hide
  the bar; a conversation deleted from `~/.claude/projects` gives the warning and a fresh
  Claude; `/clear` then close and reopen offers the new conversation; Copilot tabs show no bar.
  The `agentws-hook` installed with the app must be the new one (it sends the id).

- Changed files: the hover card lists them under `+N` and stays open while the pointer moves
  onto the list; clicking a file opens it in that tab's viewer (also with the settings page
  open, and for a non-selected tab). The explorer colors a file an agent writes within a
  second, and its parent folders; colors in light and dark themes are readable. Windows paths
  (`git rev-parse` gives `C:/...`) match the tree.

- Tab Back / Forward / Last tab: the ← → buttons dim when there is nowhere to go and their
  tooltips name the keys; Ctrl+Alt+Left/Right and Ctrl+` work from the terminal (not sent to
  the program), also on Windows (AltGr layouts) and under GNOME, whose default workspace
  switch also uses Ctrl+Alt+Left/Right; Settings → Keyboard lists and rebinds them.

- Unsaved viewer edits: opening another file asks "Save changes?"; Save writes and goes on,
  Discard drops, Cancel keeps; Save on a file changed on disk asks first, and its Cancel
  cancels. Closing the window (title bar button, Alt+F4) with edits in two tabs asks once
  (Save all saves both and closes; Discard closes; Cancel keeps all). Linux and Windows.

Remote tabs (`--remote <host>`, docs/remote.md), checked in Xvfb with a stand-in for ssh:

- Settings → Remote hosts: Add host, type the three fields (the row title and "Connects with"
  line follow), Remove host on a middle host (the fields below show the right values); a host
  edited by hand in `settings.json` shows on the page. Linux and Windows.
- `+` menu: a submenu per host with a destination, none for a host without one; Shell, Claude
  Code and Copilot CLI open on the host in its home folder, named "Shell (host)"; the window
  stays responsive while connecting (toast "Connecting to ..."), and two tabs started quickly
  on a host share one ssh connection.
- An unreachable host (wrong name, refused key, missing server program): the error toast
  shows ssh's message and stays until closed; `--remote <unreachable>` the same, after the
  local tab opens.

- Windows app to a Linux host over real ssh (`ssh.exe` with a key): the tab opens without a
  console window flashing up, typing, resizing, colors and Claude Code's full screen work;
  pulling the network shows the tab Disconnected, and `agentws-server list` on the host
  still shows the session.
- Reconnect (rebuild app and server): pulling the network (or killing the tab's `ssh`) turns
  its remote tabs Disconnected (unplug badge, banner "Connection to <host> lost: ..." with
  Reconnect, not "Process exited"); Reconnect from the banner or the tab's right-click menu
  shows "Reconnecting to <host>…", then the screen comes back and typing works, in place
  (name, color, group, focus kept), for all the host's disconnected tabs at once over one
  ssh. Host still unreachable: an error toast, and the tab can try again. A session that
  exited meanwhile shows "Process exited with code N"; one closed with `agentws-server close`
  shows "The session is gone ..." with Start new, which starts the same program in its last
  folder. Linux and Windows.

- Remote folder (rebuild app and server: protocol 2): `cd` in a remote shell tab updates the
  folder on its card and row; with it selected, Claude Code from the `+` menu, "New tab"
  (Ctrl+T) and Duplicate open on the host in that folder; a shell picked by name opens here;
  `/exit` in a remote Claude tab falls back to the host's shell in the same folder.

- Remote tab marker: a remote tab's row shows only the server icon on its second line until
  its folder is known (over a slow real ssh); a host renamed in Settings → Remote hosts shows
  its new name in the next hover card; Location off in the tab card settings hides the host
  line. On Windows: the server icon on the row and the card.

- Remote host status (rebuild app and server): over real ssh, Check on Settings → Remote
  hosts shows "Server running, N sessions" / "Server not running" with the version; a host
  that is off or unknown shows "Unreachable: ..." within ssh's 10 s connect timeout; a server
  built before `status` shows "Server too old to report status"; a daemon left running from
  an older protocol after updating the program shows "A server of another version is
  running"; an app of another protocol shows "speaks protocol N, this app M". Quitting the
  app mid-check leaves no `ssh` running past its timeout. On Windows: no console window
  flashes up during a check.

Subagent list (Claude Code 2.1.316, Copilot CLI 1.0.105):

- Real Claude Code: a background subagent (`run_in_background`) shows under the tab with its
  description while the main tab goes Idle, and goes away when it finishes; a subagent's
  permission prompt turns its row and the count ring orange. Killing a background agent from the
  task panel removes it (else note it in docs/known-limitations.md).
- Real Copilot CLI: a sync and a background `task` subagent show and go away; under
  `--autopilot` its permission requests do not mark it waiting.
- Windows: the same, with the turning count ring (its digit centered) and the list under the
  row, folded in and out by clicking the ring.

Agent running in place in a shell tab (from its `N` mark until the command's `D`):

- Linux and Windows: real Copilot CLI in place (`ConEmuANSI=ON copilot --plugin-dir
  "$AGENTWS_COPILOT_PLUGIN"` after the mark): Working spinner from progress, Waiting on a
  permission prompt, Mode line from the extension; Claude Code in Plan mode via hooks.
- zsh: the same as bash (mark by hand, status, title back at `D`, exit toast code from
  `exit=`).
- Typed `claude` / `copilot` (bash, zsh with oh-my-zsh, Git Bash on Windows): the real agents
  run in place with status (Copilot's Working spinner needs the `ConEmuANSI=ON` the function
  sets); `claude; sleep 5` shows teal Running for `sleep` once Claude exits, and a background
  tab gets one exit toast, at the agent's exit; `claude -p hi`, `claude --version`,
  `claude mcp list`, `copilot login`, `claude -p hi | cat`, `\claude` and `command claude`
  run unchanged; a `claude` alias in `~/.bashrc` / `~/.zshrc` is kept.
- Typed `claude` / `copilot` in PowerShell (pwsh 7 and Windows PowerShell 5.1, Windows and
  Linux; the pwsh tests only run where pwsh is installed, e.g. the Windows CI): the real
  `claude.exe`, npm's `claude.ps1` / `claude.cmd` shims and `copilot` run in place with status
  and see the terminal (a native program run from a function keeps the console); which of
  `claude.ps1` / `claude.cmd` `Get-Command` picks first; `claude -p hi`, `'hi' | claude -p`,
  `claude | Out-String`, `claude.exe`, `claude mcp list` run unchanged; a `claude` function
  in the profile is kept; Ctrl+C in Claude and Copilot (the X mark or the `D` ends the run,
  `ConEmuANSI` restored); a background tab's exit toast code; D's `exit=` after a failed
  program, a failed cmdlet and success; an oh-my-posh / starship prompt still shows the
  failure of the last command (`$?` restored before the user's prompt).
- Switch "Agent status for agents typed at the prompt" off (Settings → Terminal), then a new
  zsh (oh-my-zsh), PowerShell (pwsh 7 and 5.1), Git Bash and WSL bash tab: typed `claude` /
  `copilot` run unchanged with no agent icon, a `claude` alias of your own is untouched,
  `$AGENTWS_TYPED_AGENTS` / `$env:AGENTWS_TYPED_AGENTS` is unset in the shell; switched back
  on, the next new tab wraps them again and tabs already open keep their behavior.

Down enters the tab list during a search:

- Linux and Windows: type a search, press Down: the ring shows on the match and F2 renames
  it; Up in the field still steps through the matches.

Wine test runner:

- Linux with Wine: run `scripts/test-windows.sh`; the app tests finish serially with only
  `duplicate_shortcut_does_not_send_eof_to_the_original_terminal` skipped. Other crates run
  normally.

Copilot Autopilot permission hooks:

- Interactive Autopilot without `--allow-all`: check whether Copilot still asks the user. In
  `-p` mode (1.0.105) it sends `PermissionRequest` then a `permission_prompt` Notification and
  denies the tool ("no one to approve"); the app hides both under Autopilot, so a real prompt
  there would show no Waiting. Otherwise check that Autopilot stays Working without a Waiting
  triangle, toast or attention shortcut target, while a real question still waits. Check mode
  changes with the optional extension, and `--mode autopilot` / `--autopilot` launches
  without it; `--yolo` alone is not Autopilot.

Sidebar and explorer opacity:

- On a composited Windows/Linux desktop, check Dark, Light and Sepia at 100 % and below:
  normal, selected and hovered sidebar/explorer rows stay readable; the file viewer,
  Settings, title bar, menus, cards, dialogs and toasts stay opaque. Check long explorer lists.
- Recheck the reported opacity leak on the affected desktop with its platform/compositor,
  theme, opacity, blur and a screenshot; distinguish the newly translucent panels from a leak.

Windows Mica client area:

- Windows 11 build 26100: check Mica below 100 % at startup and when selected live: the
  client area must show the material. Switch Mica -> None / Acrylic / 100 % and back,
  including a restart; the previous material must disappear. Check the persistent warning if
  a DWM operation fails.
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

File viewer off the UI thread, cached (reviews/performance-review.md):

- Open a text file, an image and a Markdown file from the explorer and by Ctrl+click on
  `file:line` in the terminal: each shows without a visible empty flash, and the cursor
  lands on the line.
- While a file is open, change it from a shell tab: the viewer follows; with unsaved edits
  it keeps them, and Ctrl+S then asks before overwriting. Hold Ctrl+S: no error, the file
  holds the last text.
- With the viewer open beside a tab printing output: typing in the raw view, scrolling the
  Markdown page, the `●` mark, Rendered / Raw, the image size in the header, hover and
  selection all redraw as before; switch the theme and drag the viewer divider.
- Expand folders in the explorer (a symlinked one too), then add and remove files in them
  from a shell tab: the tree follows. Delete and recreate an expanded folder: it follows
  again.
- Release build with `lto = "thin"` and `codegen-units = 1`: build time on CI, and the
  packaged app starts.

Packaged apps on Windows and Linux: a smoke test and the About dialog's version and commit.

## Backlog

Optional behavior gets one switch per major feature, not per action (FEATURES.md, "Optional
behavior"); items below name their switch where they need one.

UX review, top 10 by value per cost ([reviews/ux-review.md](reviews/ux-review.md), IDs are
its items), regrouped into steps, in order:

1. Right-click menu in the terminal (E1): today the right button only reaches programs that
   track the mouse (`terminal_view.rs:1422`). Copy (disabled without a selection), Paste,
   Select all, Search, Clear scrollback; on a link: Open, Copy link. With mouse reporting on,
   Shift+right-click opens it. Each entry shows its shortcut.
2. Shell tab names (#4, the user chose option A on 2026-11-13): a shell row shows the
   program's title (`user@host: ~/path`), so shells in different folders look the same. With
   shell integration, name the tab by the running command, else the folder's last component;
   the program title moves to the hover card. A custom name always wins; agent tabs keep
   their titles.
3. Explorer Delete to the trash (F5): Delete is permanent today (`file_explorer.rs:1071`).
   Move to the system trash (Recycle Bin, freedesktop trash); keep the permanent delete with
   today's wording where no trash exists (network mounts, WSL paths). Check the new
   dependency with `cargo deny`.

- System notification and sound for agent attention (follow-up of #11): beside the window's
  attention request, `show_system_notification` and a sound, so the Settings / Git page in
  the active window (where the request does nothing) also gets a signal.

- Remote host status and control, in order (each ships alone):
  1. Start and Stop on Settings → Remote hosts: Start runs the daemon on the host over ssh;
     Stop (a new `agentws-server stop`) asks first when sessions still run there, listing them
     via `list`. The host's state refreshes after each.
  2. The host's last known state in its `+` submenu, with a Check entry there.

- Search a Markdown file in its rendered view: Ctrl+F while the viewer shows a rendered `.md`
  file opens a search bar over the rendered text (all matches highlighted, Enter / Shift+Enter
  to move, scrolled into view, match count, case and whole-word toggles); Esc closes it and
  returns focus to the file. Terminal shortcuts unchanged. Built first of "File viewer search
  and replace" below, whose search bar it shares.

UX review, Tier 2 (small polish, one batch):

- Shortcut hints and wording (#29, #28, #8): "Close tab" shows Ctrl+Shift+W
  (`tab_menu.rs:124`); "Copy pane title" becomes "Copy title" (`tab_menu.rs:102`); "Close
  explorer" tooltip shows its key (`file_explorer.rs:866`); "Search tabs..." gets a tooltip
  with Ctrl+Shift+S; all through `shortcuts::with_shortcut` / `menu_label_with_hint`.
  "Bowarp's history" becomes "the command history" (`settings_page.rs:536`, `:572`); the
  inline suggestions description (`settings_page.rs:561`) says the last session's command is
  accepted with Right or End only (Tab doesn't take it); the Keyboard page's '"Copy" in
  settings.json.' line (`keyboard.rs:92`) moves to a tooltip. The hover card's bare "+5"
  (`sidebar.rs:994`, `:1113`) reads "+5 lines".
- Settings layout (#25): `SettingsView::sidebar_width(px(200.))` (`settings_page.rs`) and
  a max width on text inputs so the Font family field stays inside the content column.
- "Hold Shift to select" hint (#19): on the first drag in a mouse-reporting tab per run, an
  info toast "Hold Shift to select text". One flag.

- PowerShell: after an app restart, a restored PowerShell tab doesn't show its last command
  (the restored-tab suggestion that bash, zsh and Git Bash tabs get). Find whether the
  command is never saved for PowerShell tabs or saved but not offered on restore.
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

### Remote sessions

Tabs whose programs run on another machine through `agentws-server` over ssh; design and
limitations in [docs/remote.md](docs/remote.md). Remote tabs open from the command line
(`--remote <host>`); next steps, in order:

- Attached clients in `agentws-server list`: a `clients` count in `SessionInfo` (the size of
  the session's fan-out client map), printed as a column. Changes the wire format: bump the
  protocol version. A client lost to a network drop still counts until its ssh connection
  times out on the server side (sshd's `ClientAliveInterval`, off by default).
- Reconnecting clients known by the server: the app sends a stable id (per install) and its
  machine name in `Hello`; a client attaching again with the same id replaces its lost
  connection (dropped from the session at once, counted once), and `agentws-server list` names
  the attached clients. A ping in the protocol drops a dead connection without relying on sshd.
- `agentws-server show <id>`: print the session's screen (the server's mirror, as plain text,
  `--scrollback` for the history too) without attaching, so the pty size is unchanged.
- Restore: `tabs.json` saves a remote tab's host and session id; on start, attach to it again
  (or offer to start a new one when it is gone); a host's running sessions not shown in a tab
  are listed to attach.
- Shell integration and status on the server: the server writes the integration scripts and
  sets the mark token, forwards `CommandStarted/Finished/Executed`, `AgentStarted/Finished`
  and progress as protocol events; agents get their hook settings and the server's own
  `HookServer`, whose reports are forwarded to the app.
- Git branch and `~` folder for remote tabs: the server reads the branch (`workspace-model`
  git, in the folder from `/proc`) and sends it with `FolderChanged`, and sends its home
  folder in the hello so the tab shows the folder shortened to `~`. Protocol bump; the card,
  row and hover card show them like a local tab's.
- File explorer for remote tabs: until built, the explorer shows "Files on <host> aren't
  available yet" instead of "No folder". Then a request/reply pair in the protocol (answered
  off the UI thread) for folder listings and file reads, run by `workspace-model` file tree
  on the server: first read-only (tree, git-ignored dimming, viewer), then refresh on change
  (a watcher on the server), then create, rename and delete.
- Server on Windows: build and test `agentws-server` there (named pipe, detached start).
- Packaging: ship `agentws-server` in the Linux release and document installing it on the
  server (README); a version check that names both versions when they differ.

### Git

- Safety rule: avoid actions that can cause conflicts. Block working-tree-changing actions
  (branch switching, pull, merge/rebase) while uncommitted changes exist, including staged,
  unstaged and untracked files. Explain the block; never auto-stash or discard changes.
- The Git page is the entry point for the items below. Its v1 defaults hold for them: the page
  never changes the working tree unless the action says so, actions run one at a time (no two
  `git` race for the index lock), and the repository's hooks run.
- Discard a file, hunk or line from the working tree, with a confirmation naming what is lost.
  The first action that changes the working tree.
- Switch branch from the Git page: select a local branch or check out a remote-tracking
  branch. Requires a clean working tree (safety rule above); define safeguards for active
  agents and refresh branch/status information for all affected tabs after a switch.
- Fetch / pull / push: authentication without a prompt (`GIT_TERMINAL_PROMPT=0`; rely on
  credential helpers), the safety rule for pull, conflicts, remote tracking; refresh every tab
  in the repository afterwards.
- Open the Git page on a tab's repository from its hover card's `+N`.
- Git page diff: syntax highlighting (the file viewer's tree-sitter highlighting could provide
  it), word-level highlights inside changed lines, and a side-by-side view.
- Git page, smaller items: stash, amend, a key to open the page, line staging of untracked
  files (intent-to-add), cancelling a long commit hook.

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
