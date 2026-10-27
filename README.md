# Agent Workspace

A terminal workspace for running Claude Code, Copilot CLI and shell sessions side by side, with
per-tab agent status. Feature spec: [FEATURES.md](FEATURES.md). Backlog: [BACKLOG.md](BACKLOG.md).
Known limitations: [docs/known-limitations.md](docs/known-limitations.md).

## Install on Windows

Run `agent-workspace-windows-x64-setup.exe` from the latest GitHub release. It installs per user
(no admin prompt) into `%LOCALAPPDATA%\Programs\Agent Workspace` and adds a Start Menu shortcut.
Run a newer setup to upgrade: it closes the app and overwrites that folder, so a taskbar pin keeps
opening the current version. The release zip is still there for a portable copy.

## Install on Linux

Unpack `agent-workspace-linux-x64.tar.gz` from the latest GitHub release and run its installer
(per user, no sudo):

```
tar -xzf agent-workspace-linux-x64.tar.gz
agent-workspace-linux-x64/install.sh
```

It puts the binaries in `~/.local/opt/agent-workspace` (linked from `~/.local/bin`), plus the
desktop entry and icons under `~/.local/share`. Run a newer tarball's `install.sh` to upgrade in
place, so a dock pin keeps opening the current version. `install.sh --uninstall` removes it.

## Build and run

On Ubuntu, install the Rust toolchain via `rustup` and the GPUI build dependencies (Vulkan,
X11/Wayland, fonts):

```
sudo apt install build-essential pkg-config libvulkan-dev libwayland-dev libxkbcommon-x11-dev \
  libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev libssl-dev clang cmake
```

Build the whole workspace first: `agentws-hook` must sit next to the app binary, or agent
status is disabled: the app then shows a warning that stays until closed ("Agent status is off:
..." with the cause) and quiet Claude Code tabs read "Status unavailable".

```
cargo build
cargo run -p agent-workspace
```

The first tab runs a shell by default. To start with an agent or in another folder:

```
cargo run -p agent-workspace -- [shell|claude|copilot] [project-dir]
```

Clippy, rustfmt and cargo-deny checks, locally and in CI: [docs/code-quality.md](docs/code-quality.md). Running
the Linux build and tests on a Windows machine (Docker or WSL):
[docs/linux-build-on-windows.md](docs/linux-build-on-windows.md).

## Tabs

The `+` button above the tab list opens a new tab with one of the supported shells (Bash, Zsh,
Fish, PowerShell) found on `PATH` (the default shell first, see [Settings](#settings)), Claude
Code or Copilot CLI; agents that are not installed are listed but disabled. New tabs start in the
selected tab's folder and open right below it (below a pinned tab: first after the pinned tabs);
with no tab open, they start in the last closed tab's folder, at the end of the list. The tab
named on the command line goes after the restored ones.

Closing the last tab keeps the window and its groups: the terminal area shows "No open tabs"
with a button per program of the `+` menu, in the same order (the default shell first; Tab and
Shift+Tab move between them, Enter or Space opens one), and the file explorer hides until the
next tab. Closing the window works as usual; at the next start the groups come back and the
default shell opens as the first tab.

| Keys | Action |
|---|---|
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+1..9 | Select tab 1..9 |
| Ctrl+Shift+T | New tab with the default shell, in the selected tab's folder (no tab: the last closed tab's, at the end) |
| Ctrl+Shift+D | Duplicate the selected tab: same program, its folder (also in the tab menu) |
| Ctrl+Shift+W | Close the selected tab (or the Settings row while it is selected); nothing with no tab |
| Ctrl+Shift+B | Hide or show the sidebar (also the title bar button) |
| Ctrl+Shift+S | Search tabs (shows the sidebar if hidden) |
| Ctrl+Shift+\| | Hide or show the file explorer (also the title bar button; the key that types `\|`) |
| Ctrl+Shift+A | Go to the next tab needing attention; with none, the next one that is Ready for review |
| F11 | Full screen on or off, also from a dialog (F11 is not sent to the program; on Linux the window manager applies it). Quitting in full screen starts maximized next time |

## Terminal

| Keys | Action |
|---|---|
| Ctrl+Shift+C, Ctrl+Insert | Copy the selection |
| Ctrl+Shift+V, Shift+Insert | Paste |
| Ctrl+V | Paste; sends `^V` to the program when the clipboard holds no text |
| Ctrl+Shift+F | Search terminal output and scrollback |
| Ctrl+Shift+Y / Ctrl+Shift+N | Allow / deny a program's clipboard write (while its notice shows) |
| Enter / Shift+Enter (in terminal search) | Next / previous match, wrapping around |
| Esc (while terminal search is open) | Close search and return to the terminal |
| Shift+PageUp / Shift+PageDown | Scroll the history by a page |
| Click a link | Open it (use Ctrl/Cmd+click when the program tracks the mouse) |

Ctrl+C, Tab and Shift+Tab always go to the program. When a program tracks the mouse (Claude
Code does), clicks and the wheel go to it; hold Shift to select text locally. On Linux a
selection is also copied to the primary selection, and middle-click pastes it. A wheel notch
scrolls 2 lines on Linux, and X11 touchpad scrolling is slowed the same way (Windows uses the
system's lines-per-notch setting).

When a program writes the clipboard or the primary selection (OSC 52, e.g. Copilot CLI's
copy on Windows, tmux, vim), a notice above the terminal shows the text's size and a preview,
and the write waits: Allow applies it, Deny drops it, and "Always for this tab" applies it and
every later one until the tab closes. A newer write replaces the waiting one; a background tab
with a waiting write shows Needs attention. Settings → Terminal → Clipboard → **Clipboard
writes from programs** (`clipboard_writes`: `ask`, `allow` or `deny`) can instead allow or deny
every write in every tab without asking; Deny also overrides "Always for this tab".

Terminal search matches literal text, ignoring case, across the current screen and retained
scrollback, including soft-wrapped lines. All visible matches are highlighted, with the active
match in a different color and its position shown as, for example, `3 of 17`. Enter and
Shift+Enter (or the arrow buttons) scroll to the next and previous matches. Each tab keeps its
own search; Ctrl+Shift+F refocuses it and selects the query for replacement. Closing search
removes highlights without resetting the scroll position. Results update as output arrives
and the terminal resizes. Search cannot retrieve output already evicted from scrollback or
transcript text held internally by a program; alternate-screen programs search their current
screen.

Links in the output are underlined in blue with a hand cursor on hover. Click to open a URL
(`http`, `https`, `ftp`) or an OSC 8 hyperlink in the browser, or a file path or `file://` URL
naming a file in the file viewer. When a program tracks the mouse, ordinary clicks go to it;
hold Ctrl (Cmd on macOS) to open a link instead. OSC 8 hyperlinks count only for `http`,
`https`, `ftp`, `mailto` and `file`, as their text need not show where they go. Paths are
resolved from the tab's folder (`~/` from home) and are links only when the file exists;
`src/foo.rs:42` or `src/foo.rs:42:7` opens the file at that line, in its raw view. A link the
program wrapped onto the next line counts as one.

When the program exits, a bar below the terminal says so, with its exit code (the terminal
gives up the bar's height, so no output is hidden); the history stays readable until the tab
is closed. When Claude Code or Copilot CLI quits, the tab instead starts the default shell in
the agent's last folder and becomes a shell tab (icon, status, saved as a shell); the agent's
output is not kept.

## Command history (bash, zsh, PowerShell tabs)

Settings → Terminal → Command history has three switches, all on by default and applied live
(`command_history`, `history_popup`, `inline_suggestions`; missing or non-boolean means on).
Command history off stops recording commands (memory and `history.jsonl`) and turns off the
popup, suggestions and last-command saving and offering; the shell's own history, folder
tracking, agent status and busy-close warnings are unaffected, and stored data is kept. History
popup off: no panel and Up goes to the shell (recording continues). Inline suggestions off: no
dimmed text, and Right, End and Tab go to the shell. Both need Command history and show disabled
without it, keeping their value. "Clear history" and "Clear saved session commands" (with a
confirmation) delete the stored history and each tab's saved last command; nothing is rerun.

| Keys | Action |
|---|---|
| Right / End | At the end of the command line: accept the dimmed suggestion |
| Up | Open the history popup (instead of the shell's own Up history) |
| Up / Down, Page Up / Page Down | Move through the popup |
| Typing, Backspace | Edit the command line; the popup lists the commands that still match |
| Enter, Tab | Put the selected command on the prompt, replacing the typed text (not run) |
| Delete | Remove the selected command from the history (and from `history.jsonl`) |
| Esc | Close the popup |

Commands run in any bash, zsh or PowerShell tab are kept with their folder and time in
`history.jsonl` in the config folder (latest use wins, newest 2 000). While you type, the newest
command starting with the typed text shows dimmed after the cursor. The popup lists the commands
containing the typed text (all of them on an empty prompt), newest at the bottom next to the
prompt, each with its folder; click one to put it on the prompt. Commands starting with a space
are not kept, nor bash commands that bash keeps out of its own history (`HISTCONTROL`,
`HISTIGNORE`). Up still goes to the program in full-screen programs (`less`, editors), while a
command runs, in Claude Code and Copilot tabs, and in shells without integration (fish, cmd,
WSL shells other than bash, shells started with their own arguments other than Git Bash and
WSL distros).

On restart, an integrated bash or zsh tab (including Git Bash and bash in WSL) offers its
last non-trivial command at its first empty prompt, dimmed with a fainter "last session"
hint when space allows. Right or End accepts it; Tab keeps shell completion, and Enter
never runs the suggestion. Any other line action, including opening HISTORY, dismisses it
for that session. PowerShell, cmd, agent tabs and shells without integration do not offer it.
Commands beginning with `exit`, `logout`, `clear`, `cls`, `reset`, `ls`, `ll`, `dir`, `pwd`,
`cd` or `history` do not replace the saved command. Private/unrecordable commands are never
saved. Saving to `tabs.json` is queued immediately when a command starts, without a debounce
delay; one background writer saves snapshots in order. A crash can lose an in-flight or queued
update, including a deletion. Deleting a command from HISTORY also forgets it in every tab.
Normal window close waits for pending saves and a final snapshot before releasing ownership
of the saved tabs; guest instances never write them. On Unix, every write makes
`tabs.json` owner-only (`0600`); on Windows it inherits the profile folder's permissions.

These tabs start with a small script that loads your own `~/.bashrc` / zsh startup files /
PowerShell profile and then marks the prompt (OSC 133); the scripts live in
`shell-integration/` in the config folder. zsh reads your `.zprofile` and `.zshrc` itself, from
your own `ZDOTDIR`. The marks carry a per-session token, so output that merely contains such
marks (a file you `cat`, a remote shell over ssh) is not tracked. bash needs 4.4 or later;
PowerShell needs PSReadLine. Git Bash's login shell gets the script too, which then loads your
login files (`/etc/profile`, then `~/.bash_profile`) instead of `~/.bashrc`; so does a WSL
distro whose login shell is bash (it reports Linux folders as `\\wsl.localhost\<distro>\...`).
cmd gets no script: its `PROMPT` (yours, else `$P$G`) is prefixed with a mark that reports the
folder and puts it in the title, so cmd tabs get the folder but no history and no Running
status.

The scroll bar sits in the right margin and shows while you scroll or hover it. Drag the
thumb, or click the track to jump there. It is hidden when there is no history, as on a
full-screen program's screen.

## Tab rows

Each row shows the session icon, the title, and below it the git branch (when the folder is a
repository) or the folder. The folder follows `cd` in a shell (Linux; other platforms show the
launch folder for now).

The badge on the icon shows the status. Hover a row for a moment to see its card: the status,
the full folder, the branch, the program's title, the session kind and, in a repository, the
lines added in the working tree (`+N`: changes to tracked files from `git diff HEAD` plus new
files not yet added and not ignored, refreshed at most every 5 s). No card opens while a tab
is dragged, a name is edited, or a menu or dialog is open; a card hidden that way comes back
after the pointer leaves the row and hovers it again.

Appearance → Tab hover card controls the card and its Status, Location, Changes, Program
title and Session kind pieces (all on initially). The tab name and Ctrl+number shortcut
always remain. Program title appears when a custom name hides it. Turning the card off
disables, but remembers, the piece switches; turning every piece off also hides the card.
Changes off skips `git diff`; Status off stops the card's clock. Group cards are unaffected.
Appearance → Group hover cards (on) turns the cards beside group headers off (a collapsed
group still shows its status); Activity animations (on) off makes the Running badge and a
working Copilot tab's spinner static and starts no animation timers. Both apply live and are
saved as `group_hover_cards` and `activity_animations` (missing or non-boolean means on).

### Copilot permission status (CLI 1.0.103)

Copilot CLI integration is tested against **1.0.103** (`npm install -g
@github/copilot@1.0.103`; `copilot --version` must report that version). Disable its automatic
updates with `COPILOT_AUTO_UPDATE=false` when reproducing these checks.

Each new Copilot tab mounts its own app-owned plugin with the supported
`--plugin-dir <directory>` option. The plugin registers only `PermissionRequest`, using
Copilot's PascalCase/snake_case payload format and direct `exec` command hooks.
`agentws-hook` forwards the event to that tab's local status socket: **Waiting**.
Working and Idle still come exclusively from terminal progress reports; the next progress
report observed after the permission hook starts clears Waiting, even if its state repeats.
Reports retain their I/O observation times so queued older progress cannot hide a newer
permission request. Copilot emits permission hooks before its approval rules, so an
automatically approved request can briefly report Waiting too.

Bowarp never installs or writes hook configuration into the user's `~/.copilot` or repository.
The per-tab plugin files live in the app's private runtime directory and are removed when
the session exits or its tab closes. Startup removes plugin directories left by processes
that are no longer running; live instances and unrelated files are retained. User settings,
authentication, hooks and plugins are retained. A missing helper,
an unwritable runtime directory, disabled hooks or sandbox-blocked hook
keeps the existing progress-only fallback (Working/Idle, no reliable Waiting).

Verified API sources: installed CLI **1.0.103** `--help` lists `--plugin-dir`, and its native
plugin loader accepts this manifest, `PermissionRequest` and `exec` hook with no warnings.
See GitHub's [plugin reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-plugin-reference)
(`hooks` component and read-only `--plugin-dir` mounts) and
[hook reference](https://docs.github.com/en/copilot/reference/hooks-reference)
(PascalCase payloads and direct executable hooks).

Manual validation pending: README checks 6–9 on Windows and Ubuntu (Wayland and X11),
including two simultaneous Copilot tabs, permission approval/denial, progress after Waiting,
disabled hooks and unchanged user configuration. Automated tests cover plugin generation,
argument ordering, tab routing, permission-to-Waiting and progress recovery; they do not
exercise a live Copilot permission dialog.

### Copilot mode extension

The optional Copilot CLI extension adds `Mode: Interactive`, `Mode: Plan` or `Mode: Autopilot`
to the tab's hover card. Mode is independent of activity: progress reports still determine
whether Copilot is working. This requires a Copilot CLI version exposing the extension SDK's
`session.rpc.mode.get()` and `session.mode_changed` event (available in 1.0.103).

Install it user-wide from this checkout in PowerShell:

```powershell
$destination = Join-Path $HOME '.copilot\extensions\bowarp'
New-Item -ItemType Directory -Force $destination | Out-Null
Copy-Item .\crates\app\copilot-extension\extension.mjs, .\crates\app\copilot-extension\bridge.mjs $destination
```

With a custom Copilot configuration directory, use its `extensions\bowarp` folder instead.
On Linux, copy those same two files into `~/.copilot/extensions/bowarp/`. No npm install is
needed: Copilot supplies its SDK. Restart Copilot to discover the extension. A project-local
extension with the same name takes precedence over the user-wide one.

Build and restart Agent Workspace before opening a new Copilot tab. It provides the local
pipe, tab session ID and `agentws-hook` path to the extension via `AGENTWS_PIPE`,
`AGENTWS_SESSION` and `AGENTWS_HOOK`. Outside those tabs the extension does nothing; it does
not change Copilot's mode or add agent tools. Delivery or SDK errors appear in the extension
log. Remove the two installed files to uninstall it.

Run the extension's tests with `node --test .\crates\app\copilot-extension\bridge.test.mjs`.

| Badge | Status |
|---|---|
| blue; a Copilot tab also shows a turning blue ring before the title (still with Activity animations off or on a software-rendered GPU) | Working |
| orange, plus a warning triangle before the title | Needs attention |
| green with a check | Ready for review: finished while you were on another tab (until you select it) |
| green | Idle |
| teal, pulsing for the first 10 s | Running: a command typed at the prompt of a bash, zsh or PowerShell tab has run for half a second; the card shows it and how long it has run (`Running for 2:14`, ticking while the card is open; no pulse on a software-rendered GPU) |
| light grey | Recent output (shell or Copilot) |
| none | No recent output |
| none | Status unavailable (agent status off): a quiet Claude Code tab while the status hooks could not start |
| hollow ring | Exited |

Ready for review marks an unread Working-to-Idle transition, not confirmation that the
agent's work is correct. Selecting the tab acknowledges it and restores Idle; a turn that
finishes in the selected tab stays Idle. New Working or Needs attention reports replace
the unread status. The sidebar, tab hover card, group totals and completion toast use the
same status; there is no extra completion notice.

A tab whose program rings the bell (BEL: Copilot, scripts, anything without status hooks)
while you are on another tab shows Needs attention until you select it. Claude Code tabs with
hooks ignore the bell, since their hooks already report it.

Background Claude Code and Copilot tabs show an in-app toast when they need attention, finish
a turn, or exit (an unsuccessful exit includes its code when available). An attention toast
says what the agent waits for: "Waiting for permission", "Waiting for your reply", or "Needs
attention" after a bell; hovering its text shows the key that goes to the next tab needing
attention (nothing while that action has no key). Background means the terminal is hidden:
another tab is selected or Settings is shown, not just that the keyboard is in another pane.
Repeated status reports, titles and output do not repeat the toast. Click a toast to select
its tab, expanding its group; click its close button to dismiss it without switching tabs, or
let it disappear automatically. A newer notice for the same tab replaces the previous one.
There is no sound or OS notification.

Right-click a row or use its `⋮` button for the tab menu: duplicate (Ctrl+Shift+D), pin (pinned tabs stay at the top
and are spared by "Close other tabs" / "Close tabs below"), copy branch name / pane title / working
directory, rename (Enter keeps, Escape cancels, an empty name goes back to the program's
title), move up / down, close, and a color marker. "New group with tab" and "Move to group"
put tabs in groups.

Closing a busy tab asks first: one where a shell command or a program started at the prompt
(such as vim) still runs, or whose agent is Working or waiting for an answer. Closing several
tabs (a group's, other tabs, tabs below) or the window asks once, listing every busy tab.
Close (Enter) goes ahead, Cancel (Escape) keeps everything open. Idle prompts, idle agents and
exited programs close without a question. The question is a safeguard, not a notification:
no setting turns it off.

## Search and attention

The "Search tabs..." field above the list (Ctrl+Shift+S) keeps the tabs whose name, title,
folder, branch or group name contain every word typed, ignoring case. Groups holding a match
show expanded while searching (their headers do not collapse until the search ends). Enter
selects the first match and clears the search; Escape clears it and returns to the terminal.
Up / Down in the field select the previous / next match (wrapping around) and keep the keyboard
in the field; Enter then keeps the tab moved to.
Selecting a tab the search hides (Ctrl+1..9, Ctrl+Tab, the attention jump) also clears it.

While a tab other than the selected one needs attention, a warning button with the number of
those tabs shows next to the search field. It and Ctrl+Shift+A go to the next tab needing
attention after the selected one; when no other tab needs attention, they go to the next tab
that is Ready for review. A tab in a collapsed group is revealed.

## Groups

Click a group header to collapse or expand it; a collapsed group shows how many of its tabs
need attention, are working, run a command, or are ready for review. Selecting a tab in a collapsed group
(Ctrl+1..9, Ctrl+Tab) expands it. New tabs open right below the selected tab, in its group. A tab
without its own color shows its group's color. Groups stay when their last tab leaves.

Hover any group header for its tab count and Working, Waiting and Ready for review totals, including
zero totals for an empty group. Counts cover the whole group even while searching. The card
does not show individual tabs or run a timer, and is suppressed while dragging a tab.

The group menu (right-click the header or its `⋮` button): rename, collapse, move up / down,
close its tabs, ungroup (the tabs stay, the group goes; "Delete group" when empty), color.

Drag a tab row to move it: the upper or lower half of a row drops above or below it, in that
row's block (dropping among pinned tabs pins it); the upper half of a group header drops above
the group, the lower half into it; the space below the last row drops at the end, ungrouped.

## File explorer

The file explorer (Ctrl+Shift+|, the title bar button next to the sidebar toggle, closed with
its `×`) sits between the sidebar and the terminal and shows the selected tab's folder; it
follows tab switches and `cd` (off Linux, in bash, zsh and PowerShell tabs only: their script
reports the folder at each prompt). Folders come first, then files, sorted by name, dotfiles
included. Click a folder to expand or collapse it (each root keeps its own expanded folders);
click a file to open it in the file viewer. Expanded folders refresh when their content changes.

The folder-with-an-arrow button in the explorer's header ("Parent folder (cd ..)") moves a
shell tab to the folder above the tree's, and the tree follows. zsh and PowerShell change
folder through a key binding of their script, keeping a half-typed line and redrawing the
prompt; other shells get a typed `cd` line. It is disabled while a command runs in the tab
(bash, zsh and PowerShell tabs, whose script reports it: `cd` would be typed into the
program), at a filesystem or drive root, and in Claude Code and Copilot tabs, whose tree shows
the session's own folder.

Right-click an entry for its menu: New file (in the entry's folder; Enter or clicking elsewhere
creates it, Escape cancels), cd to directory (shell tabs, at the prompt), Open in new tab (the
default shell), Open in new tab with (a submenu: Claude Code or Copilot CLI), Reveal in file
manager, Rename, Delete (asks first; deletes for good, not to the trash), Copy path, Copy
relative path (from the explorer's root). cd and the Open in new tab entries use the folder
itself, or a file's parent folder.

## File viewer

A file clicked in the explorer (or a path Ctrl+clicked in the terminal) opens in a panel right
of the terminal (closed with its `×`). Each tab has its own: selecting a tab shows the file it
had open, or no panel. Drag the divider on the panel's left edge to resize it (both the panel
and the terminal keep at least 240 px); double-click it for the default 45% of the window. The
width is the same in every tab, follows window resizes and is not saved yet.

Markdown, JSON and SVG have two views, picked in the panel's header:

- **Rendered**: Markdown as a formatted page, whose links to other files open them in the
  panel (folders in the explorer, web links in the browser); JSON laid out one member per
  line, colored, with fold arrows by the line numbers (hover them) to collapse an object or
  array. Keys keep their order and values stay as written. JSON that doesn't parse shows the
  error's line and column above the source. SVG drawn as an image.
- **Raw**: the file as written, colored and editable.

Other files open in the raw view only, colored for JSON, JavaScript, TypeScript (and TSX),
Markdown, TOML, YAML, Rust, Bash and Python, plain otherwise. Images (PNG, JPEG, GIF, BMP, WebP,
ICO, TIFF) are drawn fit to the panel, never larger than their own size, with their pixel size
in the header; one that does not decode (AVIF is not supported) shows "The image could not be
read." Files over 2 MB, other binary files and files that aren't UTF-8 are not shown.

The header's overflow menu provides Save (enabled for unsaved edits) and, for Markdown, JSON
and SVG, checked Raw/Rendered choices. The existing mode buttons and Ctrl+S still work.

| Keys | Action |
|---|---|
| Ctrl+S | Save the file (a `●` after its name marks unsaved edits) |

Opening another file or closing the panel with unsaved edits asks before dropping them. When
the file changes on disk (an agent edits it), the panel reloads it where it was scrolled to,
unless it has unsaved edits, which are kept.

## Settings

The gear button in the title bar (after the `i` button) opens the settings page in place of the
terminal, the file viewer and the file explorer and adds a Settings row under the tabs (the
viewer keeps its file and unsaved edits, the explorer its width and expanded folders; both come
back with the tab); select a tab to go back, select the row to return, and close the row with
its × or Ctrl+Shift+W. The explorer button or Ctrl+Shift+| on the page goes back to the tab with
the explorer shown. The row isn't reopened at the next start.
Clicking the gear again while the page or one of its controls has focus closes the page and
removes its row, returning focus to the terminal. If another pane has focus, the first click
focuses Settings instead.

The search box filters the rows by title, description and keywords. A change saves to
`settings.json` in the config folder at once; "Reset to default" under a changed row (or the
page's reset button) restores the default. "Open settings file" goes back to the selected tab
and shows `settings.json` in its file viewer, creating it first if it doesn't exist (with no tab
open it is disabled: there is no viewer to show it in); opening a
file from the explorer while the page shows also goes back to the tab. A banner on the page says
when the file can't be read (the defaults apply until it is fixed or a change moves it to
`settings.json.bak`) or a change couldn't be saved. The About page shows the version and the
config folder.

The Appearance page's Light theme switch changes the UI live and persists the choice
(`"ui_theme": "light"` or `"dark"` in `settings.json`, default dark). Terminal colors are
unchanged; terminal color schemes and following the OS theme are not included.

The Appearance page also sets the terminal font: its family (an installed font's name;
empty, or a font that isn't installed, uses DejaVu Sans Mono, Cascadia Mono on Windows) and
size (the slider applies while dragged and saves when released; the file accepts 6 to 72
points). Each tab re-measures its cells and resizes its terminal when it is next shown.

Appearance → Window sets the terminal background's opacity and blur, applied live:

- Opacity (`"window_opacity"`, a percent, default 100; the file accepts 30 to 100, the
  slider steps by 5 and saves when released): below 100 % the desktop shows through the
  terminal's background and padding. Text, cells a program colored, the selection, the cursor
  and the rest of the window (title bar, sidebar, settings, file panes, menus) stay opaque.
- Background blur (`"window_blur"`: `"none"` (default), `"blur"` or `"mica"`): what shows
  through. On Windows: None, Acrylic (`"blur"`) or Mica, which needs Windows 11 22H2 or
  later (Acrylic is used before). On Linux: None or Blur, on KDE Plasma (Wayland) only;
  elsewhere (GNOME, X11) the opacity still applies and the dropdown is disabled ("Not
  available on this system"). A saved `"mica"` means Blur on Linux. It needs an opacity
  below 100 %.

The Terminal page sets the default shell: the one the `+` menu lists first and Ctrl+Shift+T,
"Open in new tab" and an agent tab that quits open, and the first tab when no tabs are saved.
"Automatic" (the default) is `$SHELL` (Git Bash on Windows, when installed; when `$SHELL` isn't
in the list, Ctrl+Shift+T opens the first shell listed); the list holds the shells found at
startup. Tabs already open keep their shell. One that isn't found gets the automatic shell.
If the first tab's requested shell cannot start but the fallback does, a warning identifies
the requested shell, retains the original failure reason and says a fallback started; it
stays until dismissed.

`settings.json` saves the shell's launcher id, which later per-launcher settings use too:
`bash`, `zsh`, `fish`, `pwsh` (PowerShell 7) on Linux and macOS; `pwsh`, `powershell`
(Windows PowerShell), `cmd`, `git-bash` and `wsl:<distro>` (e.g. `wsl:Ubuntu`) on Windows; the
agents are `claude` and `copilot`. Written by hand, the id's case doesn't matter, and a menu
name (`Zsh`, `Git Bash`, `Ubuntu`) works too.

The Terminal page also shows each agent's executable path and a free-text arguments field.
`claude_args` and `copilot_args` in `settings.json` default to empty. Quotes group words
(for example `--model opus`, `--permission-mode plan` or `--dangerously-skip-permissions`
for Claude; `--mode autopilot`, `--mode plan` or `--yolo` for Copilot). Copilot's mode is
separate from permission bypass. Parsing uses shell-style quoting and backslash escaping,
without variable expansion or executing a shell; quote Windows paths with single quotes
or escape their backslashes. Unmatched quotes prevent launching and show an error.

These words follow the app's own arguments, including Claude's hooks `--settings`; a user
`--settings` is passed through unchanged, even if it overrides the hooks. The + menu and
explorer agent launches always use the current setting (Ctrl+Shift+T still opens the default
shell). Existing, duplicated and restored tabs keep their original argument vector, saved
in `tabs.json`; changing the setting only affects new launches.

Hover-card settings are saved as independent booleans, for example:

```json
"tab_card": {
  "enabled": true,
  "status": true,
  "location": true,
  "changes": false,
  "program_title": true,
  "session_kind": true
}
```

Missing or wrong-typed booleans each default to true. Changes apply live.

### Workspace switches

Settings → Workspace has three switches, all on by default (`restore_tabs`,
`remember_last_command`, `file_panes`; missing or non-boolean means on).

- **Restore previous tabs** off: the next start opens one fresh default tab, and `tabs.json`
  is neither read nor written (it is kept as it is, so turning the switch on again restores
  those tabs at a later start, until the next save replaces them). Loading takes effect at the
  next start; saving stops at once, and starts again when the switch is turned back on.
  Window size and position, settings and history are separate files and are not affected.
- **Remember last shell command** off: no new last command is captured or saved, and a saved one
  is not offered; saved values already in `tabs.json` are kept, not erased. It needs Command
  history and Restore previous tabs (shown disabled with a caption naming the first one that is
  off, value kept), and showing the suggestion also needs Inline suggestions. Applies live.
  Folder tracking, agent status and busy-close warnings are unaffected.
- **File explorer and viewer** off (applies live): both panes are hidden, the title bar button
  goes and Ctrl+Shift+| does nothing, no folder is scanned or watched and no file loaded, and
  every open file is closed. When a file has unsaved edits, the viewer's Discard/Cancel
  question comes first (one per file); Cancel keeps the switch on. A Ctrl+clicked file link
  asks "Open externally?" and opens the file with the system's application; "Open settings
  file" does the same. The pane widths (`file_viewer_fraction`, `file_explorer_width`) are kept.
  Turned on again, the panes work as before; files closed meanwhile stay closed.

### Notifications

Settings → Notifications has one switch, **In-app notifications** (`in_app_notifications`, on
by default; missing or non-boolean means on). Off (applies live): no toast appears when a
background tab's agent needs attention, is ready for review or exits cleanly; the sidebar
status, status dots and attention badges are unchanged, and a toast already on screen stays
until it hides. An agent that exits with a nonzero or unknown exit code still shows its toast.
Toasts that answer something you did (save or delete errors, history cleared, failed starts)
and the persistent fallback-shell warning always show.

### Keyboard shortcuts

The `"keybindings"` object in `settings.json` changes the default shortcuts key by key: an
action name binds the key (replacing its default), `null` unbinds a default key so it goes to
the program. For example, to get `^V` back for bash's quoted-insert and vim's visual block and
paste with Ctrl+Shift+V only:

```json
"keybindings": { "ctrl-v": null, "ctrl-alt-t": "NewTab" }
```

Keys are written as GPUI keystrokes: modifiers `ctrl`, `alt`, `shift`, `super` (Windows key)
joined by `-` to a key (`a`, `f5`, `tab`, `escape`, `insert`, `pageup`; a capital letter
adds Shift: `ctrl-V` is `ctrl-shift-v`); a space separates the keystrokes of a sequence (`ctrl-k ctrl-c`). Actions: `Copy`, `CopyOrSend`,
`Paste`, `PasteOrSend`, `Find`, `CloseFind`, `AllowClipboard`, `DenyClipboard` (in the
terminal); `NewTab`, `DuplicateTab`, `CloseTab`, `NextTab`, `PreviousTab`, `SelectTab1` to
`SelectTab9`, `ToggleSidebar`, `ToggleExplorer`, `SearchTabs`, `NextAttention`,
`ToggleFullScreen` (anywhere in the window). `terminal::Paste` and `workspace::NewTab` also
work. An entry that can't be used (an unknown action, a key that can't be read, a value that
is neither a name nor `null`, `null` for a key with no default) is skipped and named in a
banner on the settings page; the others apply. A change applies when the file is read again:
when the settings page opens, when the file is saved in the file viewer, or at the next
change on the page. Ctrl+C, Tab and Shift+Tab bound to a workspace action still go to the
program while the terminal has the keyboard. Tooltips and menus show the shortcuts in use.

The settings page's Keyboard page edits the same object: one row per action with its keys.
Click a key and press the new shortcut to replace it, `+` to add one, `×` to remove one, and
"Reset to default" (or "Reset all" at the end) to go back to the defaults. Recording takes a
single keystroke (plain Esc cancels). Keys that terminal programs use are refused with a toast:
typing keys (letters, Space, Enter, arrows) and Ctrl or Alt plus a letter (Ctrl+D, Alt+B); use
Ctrl+Shift, Super or a key like F5.
A shortcut already in use moves to the new action, and a toast names the action that had it.
Entries the app can't read are left in the file.

## About

The `i` button in the title bar (after the explorer button) shows the version, with the git
commit it was built from when built from a checkout, and a link to the repository. Copy version
copies the whole line for a bug report (or select part of it and press Ctrl+C); Escape or Close
dismisses it.

The version is the workspace `version` in `Cargo.toml`. `scripts/bump-version.sh
[patch|minor|major]` (default `patch`) raises it, refreshes `Cargo.lock` and prints the new
version; it refuses a `Cargo.toml` with uncommitted changes. Each pushed feature is one patch
higher and tagged `v<version>`; finishing a milestone bumps `minor`.

## Design decisions

- **Quitting an agent:** when Claude Code or
  Copilot CLI exits in its tab with code 0, the tab starts a new default shell in the agent's
  last folder and becomes a shell tab; the agent's output is not kept. When it exits with an
  error, the tab stays as it was ("Exited"), so the error can be read. The agent keeps
  running directly (no shell around it), which:
  - starts faster: no shell startup files (`.bashrc`, `nvm`, `conda`) run before it;
  - uses less memory: no idle shell waiting behind every agent tab;
  - works the same with every default shell, `cmd` and shells without integration included,
    with no command quoting through a shell (fewer Windows pitfalls);
  - keeps a tab's kind fixed while the agent runs, so status and icon need no tracking of what
    runs in it.

  Rejected: launching the agent inside
  the default shell (as if typed at a bash prompt), which would keep the screen and the
  `claude --resume <id>` line and give the agent your shell startup environment, but needs a
  run-first step in every shell integration script and a tab kind that changes over time.

## Manual check

Open 3 Claude Code tabs and 1 shell tab, then check:

1. **Typing and resizing** are smooth in every tab.
2. **Working:** give a Claude tab a prompt and switch to another tab. Its badge turns blue.
3. **Needs attention:** ask a Claude tab for something that needs a permission prompt and switch
   away. Its badge turns orange and a warning triangle shows before the title.
4. **Ready for review / Idle:** give a Claude tab a short prompt (`say hi in one word`) and switch away right
   away. The badge goes blue, then green with a check when the reply finishes; selecting the
   tab turns it into a plain green Idle badge. Confirm "Ready for review" in its hover card,
   group totals and single completion toast; dismissing the toast must not clear the status.
   Repeat with Copilot. Finish a turn without switching away and confirm Idle with no toast;
   start new work or request permission in an unread tab and confirm Working or Needs attention
   replaces Ready for review. If you interrupt a turn with Esc instead, the
   tab stays blue for about a minute, because Claude Code fires no hook on interrupt.
5. **Activity badge:** in the shell tab run

   ```
   sleep 5; for i in $(seq 1 10); do echo tick $i; sleep 1; done
   ```

   and switch to another tab within 5 s. The shell's badge is light grey while ticks print and
   disappears about 2 s after the last one.
6. **Folder and branch:** in a shell tab inside a repository the second line shows the branch;
   `cd /tmp` changes it to `/tmp` once the prompt returns.
7. **Tab menu:** pin the last tab (it moves to the top with a pin icon), rename one, give one a
   color, move one up, then "Close other tabs" on an unpinned tab: the pinned tab stays.
8. **Groups:** "New group with tab" on a Claude tab and name it, drag a second tab onto the
   group header's lower half, collapse the group while one of its tabs is working (the header
   shows a working chip), then press that tab's Ctrl+number: the group expands.
9. **Search and attention:** type part of a folder or branch name in "Search tabs..." and press
   Enter: that tab is selected. Get a Claude tab to a permission prompt in a collapsed group,
   select another tab and press Ctrl+Shift+A: the group expands and the tab is selected. Hover
   a row in a repository with local changes: the card shows `+N`.

Last passed: Ubuntu, Wayland, 2026-10-17 (checks 1–5; checks 6–9 were added later and are not recorded yet).

### Six UX improvements (manual checks pending)

1. Open Settings, focus a field, then click the gear: its row disappears and terminal input
   works again. With Settings shown, focus the file viewer; the first gear click focuses
   Settings, and the second closes it.
2. Open a Markdown, JSON or SVG file. Use the overflow menu to switch Raw/Rendered, edit in
   Raw, and Save. Confirm the check marks, disabled Save for a clean file, the existing mode
   buttons and Ctrl+S; images must not offer Raw/Rendered or an enabled Save.
3. Configure a discovered startup shell whose executable cannot start, with a working
   automatic fallback. Start with no restored tabs: the fallback opens, and the persistent
   warning identifies the requested shell and original error, and says a fallback started.
4. Switch Light theme on and off; check sidebar, Settings, menus, viewer and hover-card
   contrast while the terminal palette stays unchanged. Restart to check persistence; also
   edit `ui_theme` in the settings file and save it in the viewer.
5. Hover expanded, collapsed and empty groups, including while filtering tabs. Confirm total
   tab count and Working/Waiting/Ready for review counts, and no group card during a tab drag.
6. Run Claude Code and Copilot in background tabs through waiting and finished states.
   Confirm one toast per transition, none for foreground turns or ordinary output, notices
   while Settings hides the terminal, click-to-select (including collapsed groups), close
   without selecting, auto-hide, and no sound or OS notices. Check successful and failed
   agent exits too.
