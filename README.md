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

## Status dots

Hover a tab row to see the status as text.

| Dot | Status |
|---|---|
| blue | Working |
| orange | Needs attention |
| green | Idle |
| light grey | Recent output (shell or Copilot) |
| dark grey | No recent output |
| hollow ring | Exited |

## Manual check

Open 3 Claude Code tabs and 1 shell tab, then check:

1. **Typing and resizing** are smooth in every tab.
2. **Working:** give a Claude tab a prompt and switch to another tab. Its dot turns blue.
3. **Needs attention:** ask a Claude tab for something that needs a permission prompt and switch
   away. Its dot turns orange.
4. **Idle:** give a Claude tab a short prompt (`say hi in one word`) and switch away right away.
   The dot goes blue, then green when the reply finishes. Let it finish: after Esc the tab
   stays blue for about a minute, because Claude Code fires no hook on interrupt.
5. **Activity dot:** in the shell tab run

   ```
   sleep 5; for i in $(seq 1 10); do echo tick $i; sleep 1; done
   ```

   and switch to another tab within 5 s. The shell's dot is light grey while ticks print and
   turns dark grey about 2 s after the last one.

Last passed: Ubuntu, Wayland, 2026-10-17 (all five checks).
