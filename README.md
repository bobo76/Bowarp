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
| Ctrl+Shift+T | New tab running what the selected tab runs, in its folder |
| Ctrl+Shift+W | Close the selected tab |
| Ctrl+Shift+B | Hide or show the sidebar (also the title bar button) |

## Terminal

| Keys | Action |
|---|---|
| Ctrl+Shift+C, Ctrl+Insert | Copy the selection |
| Ctrl+Shift+V, Shift+Insert | Paste |
| Shift+PageUp / Shift+PageDown | Scroll the history by a page |

Ctrl+C, Tab and Shift+Tab always go to the program. When a program tracks the mouse (Claude
Code does), clicks and the wheel go to it; hold Shift to select text locally. On Linux a
selection is also copied to the primary selection, and middle-click pastes it.

The scroll bar sits in the right margin and shows while you scroll or hover it. Drag the
thumb, or click the track to jump there. It is hidden when there is no history, as on a
full-screen program's screen.

## Tab rows

Each row shows the session icon, the title, and below it the git branch (when the folder is a
repository) or the folder. The folder follows `cd` in a shell (Linux; other platforms show the
launch folder for now).

The badge on the icon shows the status; hover the icon to see it as text.

| Badge | Status |
|---|---|
| blue | Working |
| orange, plus a warning triangle before the title | Needs attention |
| green with a check | Done: finished while you were on another tab (until you select it) |
| green | Idle |
| light grey | Recent output (shell or Copilot) |
| none | No recent output |
| hollow ring | Exited |

Right-click a row or use its `⋮` button for the tab menu: pin (pinned tabs stay at the top
and are spared by "Close other tabs" / "Close tabs below"), copy branch name / pane title / working
directory, rename (Enter keeps, Escape cancels, an empty name goes back to the program's
title), move up / down, close, and a color marker. "New group with tab" and "Move to group"
put tabs in groups.

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

Last passed: Ubuntu, Wayland, 2026-10-17 (checks 1–5; checks 6–8 were added later and are not recorded yet).
