# Agent Workspace

A terminal workspace for running Claude Code, Copilot CLI and shell sessions side by side, with
per-tab agent status. Feature spec: [FEATURES.md](FEATURES.md). Plan and status: [PLAN.md](PLAN.md).

## Build and run

Build the whole workspace first: `agentws-hook` must sit next to the app binary, or agent
status is disabled.

```
cargo build
cargo run -p agent-workspace
```

Ubuntu build dependencies are listed in [PLAN.md](PLAN.md#ubuntu-prerequisites).

## Tab rows

Each row shows the session icon, the title, and below it the git branch (when the folder is a
repository) or the folder. The folder follows `cd` in a shell (Linux; other platforms show the
launch folder for now). Ctrl+Shift+B or the title bar button hides the sidebar.

Right-click a row or use its `⋮` button for the tab menu: pin (pinned tabs stay at the top
and are spared by "Close other tabs" / "Close tabs below"), copy branch / pane title / working
directory, rename (Enter keeps, Escape cancels, an empty name goes back to the program's
title), move up / down, close, and a color marker.

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

## Manual check

Open 3 Claude Code tabs and 1 shell tab, then check:

1. **Typing and resizing** are smooth in every tab.
2. **Working:** give a Claude tab a prompt and switch to another tab. Its badge turns blue.
3. **Needs attention:** ask a Claude tab for something that needs a permission prompt and switch
   away. Its badge turns orange and a warning triangle shows before the title.
4. **Done / Idle:** give a Claude tab a short prompt (`say hi in one word`) and switch away right
   away. The badge goes blue, then green with a check when the reply finishes; selecting the
   tab turns it into a plain green Idle badge. Let it finish: after Esc the tab stays blue for
   about a minute, because Claude Code fires no hook on interrupt.
5. **Activity dot:** in the shell tab run

   ```
   sleep 5; for i in $(seq 1 10); do echo tick $i; sleep 1; done
   ```

   and switch to another tab within 5 s. The shell's badge is light grey while ticks print and
   disappears about 2 s after the last one.
6. **Folder and branch:** in a shell tab inside a repository the second line shows the branch;
   `cd /tmp` changes it to `/tmp` once the prompt returns.
7. **Tab menu:** pin the last tab (it moves to the top with a pin icon), rename one, give one a
   color, move one up, then "Close other tabs" on an unpinned tab: the pinned tab stays.

Last passed: Ubuntu, Wayland, 2026-10-17 (all five checks).
