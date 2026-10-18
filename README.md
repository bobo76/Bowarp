# Agent Workspace

A terminal workspace for running Claude Code, Copilot CLI and shell sessions side by side, with
per-tab agent status. Feature spec: [FEATURES.md](FEATURES.md). Plan and status: [PLAN.md](PLAN.md).
Architecture: [ARCHITECTURE.md](ARCHITECTURE.md).

## Build and run

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

Ubuntu build dependencies are listed in [PLAN.md](PLAN.md#ubuntu-prerequisites).

## Tabs

The `+` button above the tab list opens a new tab with one of the supported shells (Bash, Zsh,
Fish, PowerShell) found on `PATH` (default shell first), Claude Code or Copilot CLI; agents that are not installed are listed
but disabled. New tabs start in the selected tab's folder.

| Keys | Action |
|---|---|
| Ctrl+Tab / Ctrl+Shift+Tab | Next / previous tab |
| Ctrl+1..9 | Select tab 1..9 |
| Ctrl+Shift+T | New tab with the default shell, in the selected tab's folder |
| Ctrl+Shift+D | Duplicate the selected tab: same program, its folder (also in the tab menu) |
| Ctrl+Shift+W | Close the selected tab |
| Ctrl+Shift+B | Hide or show the sidebar (also the title bar button) |
| Ctrl+Shift+S | Search tabs (shows the sidebar if hidden) |
| Ctrl+Shift+\| | Hide or show the file explorer (also the title bar button; the key that types `\|`) |
| Ctrl+Shift+A | Go to the next tab needing attention; with none, the next one that is Done |

## Terminal

| Keys | Action |
|---|---|
| Ctrl+Shift+C, Ctrl+Insert | Copy the selection |
| Ctrl+Shift+V, Shift+Insert | Paste |
| Shift+PageUp / Shift+PageDown | Scroll the history by a page |

Ctrl+C, Tab and Shift+Tab always go to the program. When a program tracks the mouse (Claude
Code does), clicks and the wheel go to it; hold Shift to select text locally. On Linux a
selection is also copied to the primary selection, and middle-click pastes it.

## Command history (bash, zsh, PowerShell tabs)

| Keys | Action |
|---|---|
| Right / End | At the end of the command line: accept the dimmed suggestion |
| Up | Open the history popup (instead of the shell's own Up history) |
| Up / Down, Page Up / Page Down | Move through the popup |
| Typing, Backspace | Edit the command line; the popup lists the commands that still match |
| Enter | Put the selected command on the prompt, replacing the typed text (not run) |
| Esc | Close the popup |

Commands run in any bash, zsh or PowerShell tab are kept with their folder and time in
`history.jsonl` in the config folder (latest use wins, newest 10 000). While you type, the newest
command starting with the typed text shows dimmed after the cursor. The popup lists the commands
containing the typed text (all of them on an empty prompt), newest at the bottom next to the
prompt, each with its folder; click one to put it on the prompt. Commands starting with a space
are not kept, nor bash commands that bash keeps out of its own history (`HISTCONTROL`,
`HISTIGNORE`). Up still goes to the program in full-screen programs (`less`, editors), while a
command runs, in Claude Code and Copilot tabs, and in shells without integration (fish, cmd,
WSL, shells started with their own arguments).

These tabs start with a small script that loads your own `~/.bashrc` / zsh startup files /
PowerShell profile and then marks the prompt (OSC 133); the scripts live in
`shell-integration/` in the config folder. zsh reads your `.zprofile` and `.zshrc` itself, from
your own `ZDOTDIR`. The marks carry a per-session token, so output that merely contains such
marks (a file you `cat`, a remote shell over ssh) is not tracked. bash needs 4.4 or later;
PowerShell needs PSReadLine and is not integrated on Windows yet.

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
| light grey | Recent output (shell or Copilot) |
| none | No recent output |
| hollow ring | Exited |

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
Selecting a tab the search hides (Ctrl+1..9, Ctrl+Tab, the attention jump) also clears it.

While a tab other than the selected one needs attention, a warning button with the number of
those tabs shows next to the search field. It and Ctrl+Shift+A go to the next tab needing
attention after the selected one; when no other tab needs attention, they go to the next tab
that is Done. A tab in a collapsed group is revealed.

## Groups

Click a group header to collapse or expand it; a collapsed group shows how many of its tabs
need attention, are working, or are done. Selecting a tab in a collapsed group (Ctrl+1..9,
Ctrl+Tab) expands it. New tabs join the selected tab's group. A tab without its own color shows
its group's color. Groups stay when their last tab leaves.

The group menu (right-click the header or its `⋮` button): rename, collapse, move up / down,
close its tabs, ungroup (the tabs stay, the group goes; "Delete group" when empty), color.

Drag a tab row to move it: the upper or lower half of a row drops above or below it, in that
row's block (dropping among pinned tabs pins it); the upper half of a group header drops above
the group, the lower half into it; the space below the last row drops at the end, ungrouped.

## File explorer

The file explorer (Ctrl+Shift+|, the title bar button next to the sidebar toggle, closed with
its `×`) sits between the sidebar and the terminal and shows the selected tab's folder; it
follows tab switches and `cd`. Folders come first, then files, sorted by name, dotfiles
included. Click a folder to expand or collapse it (each root keeps its own expanded folders);
click a file to open it in the file viewer. Expanded folders refresh when their content changes.

Right-click an entry for its menu: New file (in the entry's folder; Enter or clicking elsewhere
creates it, Escape cancels), cd to directory (shell tabs), Open in new tab (the default shell),
Reveal in file manager, Rename, Delete (asks first; deletes for good, not to the trash), Attach
as context (types the path at the selected tab's prompt without Enter: `@path` in Claude Code,
the shell-quoted path elsewhere), Copy path, Copy relative path (from the explorer's root).
cd and Open in new tab use the folder itself, or a file's parent folder.

## File viewer

A file clicked in the explorer opens in a panel right of the terminal (closed with its `×`).
Markdown and JSON have two views, picked in the panel's header:

- **Rendered**: Markdown as a formatted page; JSON laid out one member per line, colored, with
  fold arrows by the line numbers (hover them) to collapse an object or array. Keys keep their
  order and values stay as written. JSON that doesn't parse shows the error's line and column
  above the source.
- **Raw**: the file as written, colored and editable.

Other files open in the raw view only, colored for JSON, JavaScript, TypeScript (and TSX),
Markdown, TOML, YAML, Rust, Bash and Python, plain otherwise. Files over 2 MB, binary files and
files that aren't UTF-8 are not shown.

| Keys | Action |
|---|---|
| Ctrl+S | Save the file (a `●` after its name marks unsaved edits) |

Opening another file or closing the panel with unsaved edits asks before dropping them. When
the file changes on disk (an agent edits it), the panel reloads it, unless it has unsaved edits,
which are kept.

## About

The `i` button in the title bar (after the explorer button) shows the version, with the git
commit it was built from when built from a checkout, and a link to the repository. Copy version
copies the whole line for a bug report (or select part of it and press Ctrl+C); Escape or Close
dismisses it.

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
