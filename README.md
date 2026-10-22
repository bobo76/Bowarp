# Agent Workspace

A terminal workspace for running Claude Code, Copilot CLI and shell sessions side by side, with
per-tab agent status. Feature spec: [FEATURES.md](FEATURES.md). Plan and status: [PLAN.md](PLAN.md).

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
status is disabled.

```
cargo build
cargo run -p agent-workspace
```

The first tab runs a shell by default. To start with an agent or in another folder:

```
cargo run -p agent-workspace -- [shell|claude|copilot] [project-dir]
```

Linting with Clippy, locally and in CI: [docs/code-quality.md](docs/code-quality.md). Running
the Linux build and tests on a Windows machine (Docker or WSL):
[docs/linux-build-on-windows.md](docs/linux-build-on-windows.md).

## Tabs

The `+` button above the tab list opens a new tab with one of the supported shells (Bash, Zsh,
Fish, PowerShell) found on `PATH` (the default shell first, see [Settings](#settings)), Claude
Code or Copilot CLI; agents that are not installed are listed but disabled. New tabs start in the selected tab's folder.

| Keys | Action |
|---|---|
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+1..9 | Select tab 1..9 |
| Ctrl+Shift+T | New tab with the default shell, in the selected tab's folder |
| Ctrl+Shift+D | Duplicate the selected tab: same program, its folder (also in the tab menu) |
| Ctrl+Shift+W | Close the selected tab (or the Settings row while it is selected) |
| Ctrl+Shift+B | Hide or show the sidebar (also the title bar button) |
| Ctrl+Shift+S | Search tabs (shows the sidebar if hidden) |
| Ctrl+Shift+\| | Hide or show the file explorer (also the title bar button; the key that types `\|`) |
| Ctrl+Shift+A | Go to the next tab needing attention; with none, the next one that is Done |
| F11 | Full screen on or off, also from a dialog (F11 is not sent to the program; on Linux the window manager applies it). Quitting in full screen starts maximized next time |

## Terminal

| Keys | Action |
|---|---|
| Ctrl+Shift+C, Ctrl+Insert | Copy the selection |
| Ctrl+Shift+V, Shift+Insert | Paste |
| Ctrl+Shift+F | Search terminal output and scrollback |
| Enter / Shift+Enter (in terminal search) | Next / previous match, wrapping around |
| Esc (while terminal search is open) | Close search and return to the terminal |
| Shift+PageUp / Shift+PageDown | Scroll the history by a page |
| Click a link | Open it (use Ctrl/Cmd+click when the program tracks the mouse) |

Ctrl+C, Tab and Shift+Tab always go to the program. When a program tracks the mouse (Claude
Code does), clicks and the wheel go to it; hold Shift to select text locally. On Linux a
selection is also copied to the primary selection, and middle-click pastes it. A wheel notch
scrolls 2 lines on Linux, and X11 touchpad scrolling is slowed the same way (Windows uses the
system's lines-per-notch setting).

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
files not yet added and not ignored, refreshed at most every 5 s).

| Badge | Status |
|---|---|
| blue | Working |
| orange, plus a warning triangle before the title | Needs attention |
| green with a check | Done: finished while you were on another tab (until you select it) |
| green | Idle |
| teal, pulsing for the first 10 s | Running: a command typed at the prompt of a bash, zsh or PowerShell tab has run for half a second; the card shows it and how long it has run (`Running for 2:14`, ticking while the card is open; no pulse on a software-rendered GPU) |
| light grey | Recent output (shell or Copilot) |
| none | No recent output |
| hollow ring | Exited |

A tab whose program rings the bell (BEL: Copilot, scripts, anything without status hooks)
while you are on another tab shows Needs attention until you select it. Claude Code tabs with
hooks ignore the bell, since their hooks already report it.

Right-click a row or use its `⋮` button for the tab menu: duplicate (Ctrl+Shift+D), pin (pinned tabs stay at the top
and are spared by "Close other tabs" / "Close tabs below"), copy branch name / pane title / working
directory, rename (Enter keeps, Escape cancels, an empty name goes back to the program's
title), move up / down, close, and a color marker. "New group with tab" and "Move to group"
put tabs in groups.

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
that is Done. A tab in a collapsed group is revealed.

## Groups

Click a group header to collapse or expand it; a collapsed group shows how many of its tabs
need attention, are working, run a command, or are done. Selecting a tab in a collapsed group
(Ctrl+1..9, Ctrl+Tab) expands it. New tabs join the selected tab's group. A tab without its own
color shows its group's color. Groups stay when their last tab leaves.

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

| Keys | Action |
|---|---|
| Ctrl+S | Save the file (a `●` after its name marks unsaved edits) |

Opening another file or closing the panel with unsaved edits asks before dropping them. When
the file changes on disk (an agent edits it), the panel reloads it where it was scrolled to,
unless it has unsaved edits, which are kept.

## Settings

The gear button in the title bar (after the `i` button) opens the settings page in place of the
terminal and adds a Settings row under the tabs; select a tab to go back, select the row to
return, and close the row with its × or Ctrl+Shift+W. The row isn't reopened at the next start.
The search box filters the rows by title, description and keywords. A change saves to
`settings.json` in the config folder at once; "Reset to default" under a changed row (or the
page's reset button) restores the default. "Open settings file" shows `settings.json` in the
file viewer, creating it first if it doesn't exist. A banner on the page says when the file
can't be read (the defaults apply until it is fixed or a change moves it to
`settings.json.bak`) or a change couldn't be saved. The About page shows the version and the
config folder.

The Appearance page sets the terminal font: its family (an installed font's name; empty, or a
font that isn't installed, uses DejaVu Sans Mono, Cascadia Mono on Windows) and size (the
slider applies while dragged and saves when released; the file accepts 6 to 72 points). Each
tab re-measures its cells and resizes its terminal when it is next shown.

The Terminal page sets the default shell: the one the `+` menu lists first and Ctrl+Shift+T,
"Open in new tab" and an agent tab that quits open, and the first tab when no tabs are saved.
"Automatic" (the default) is `$SHELL` (Git Bash on Windows, when installed; when `$SHELL` isn't
in the list, Ctrl+Shift+T opens the first shell listed); the list holds the shells found at
startup. Tabs already open keep their shell. One that isn't found gets the automatic shell.

`settings.json` saves the shell's launcher id, which later per-launcher settings use too:
`bash`, `zsh`, `fish`, `pwsh` (PowerShell 7) on Linux and macOS; `pwsh`, `powershell`
(Windows PowerShell), `cmd`, `git-bash` and `wsl:<distro>` (e.g. `wsl:Ubuntu`) on Windows; the
agents are `claude` and `copilot`. Written by hand, the id's case doesn't matter, and a menu
name (`Zsh`, `Git Bash`, `Ubuntu`) works too.

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
4. **Done / Idle:** give a Claude tab a short prompt (`say hi in one word`) and switch away right
   away. The badge goes blue, then green with a check when the reply finishes; selecting the
   tab turns it into a plain green Idle badge. If you interrupt a turn with Esc instead, the
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
