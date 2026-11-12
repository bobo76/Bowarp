# Claude tab lags 2–3 seconds

## Symptom

In a Claude Code tab, the app responds 2–3 seconds late, then catches up:

- Selecting text: nothing is highlighted, then 2–3 s later the selection shows and the text is
  copied.
- Typing: the characters show up 2–3 s after the keys are pressed.
- The rest of the app still works: you can switch to another tab.

## What the symptom means (corrected 2026-11-12)

The first version of this note said the selection is drawn by the app, so the delay must be in
the app. That is wrong for this setup:

- `~/.claude/settings.json` has `"tui": "fullscreen"`. In that mode Claude Code turns mouse
  reporting on, so the app forwards the mouse to Claude (`pointer.rs`, `on_mouse_down`:
  `mouse::is_reporting_to_program`) and draws no selection of its own.
- On Windows the app never copies on select (`on_mouse_up` only writes the primary selection
  on Linux). "The text is copied" is Claude copying its own selection (OSC 52).

So both symptoms are the same round trip: input → pty → Claude → output → paint. The delay
can be in any of:

- **(a)** the app writing input to the pty;
- **(b)** Claude Code or the Windows console host (inbox `conhost.exe`; no `conpty.dll` is
  shipped next to the app);
- **(c)** the app reading the output and painting it;
- **(d)** the app's main thread being busy (then the whole window lags, not just the tab).

## Checked, found clean (code reading, not measured)

- **Per-wakeup work** (`on_wakeup`, `refresh_history`, `take_snapshot`): for a Claude tab it
  is a few short terminal locks and a copy of the visible cells. Search only runs with the
  search bar open; link lookup only with Ctrl held.
- **Terminal lock.** The I/O thread holds the fair lock's lease for one `pty_read` (at most
  64 KB parsed), so a key press or paint waits milliseconds, not seconds.
- **Wakeup re-arming.** Every path re-arms (`take_snapshot`, `hide`, hidden `on_wakeup`).
  alacritty skips the wakeup only while a synchronized update is still open, and wakes when
  it ends or after 150 ms.
- **Shell-integration mark filter** (`MarkFilter`, its 1 ms sleep and Windows `Wake`): off
  for Claude tabs (no mark token).
- **Hooks.** `agentws-hook.exe` takes about 100 ms per run here (measured, 20 runs). Typing
  and selecting fire no hook anyway.
- **Git branch read, explorer sync** on tab changes: a few small file reads.
- **CPU.** The app used about 10% of one core over 10 minutes with 8 Claude tabs open: its
  main thread is not saturated on average (a short stall would not show in this number).

## Found, but does not explain this symptom

- **Output can stall until the next input (Windows, unverified).** `pty_read` stops after
  64 KB (`MAX_LOCKED_READ`). alacritty's Windows pipe reader only signals new output after a
  read found the pipe empty, so after a read that stopped at the cap, later output raises no
  event. Any key or mouse report fixes it at once (the write re-registers the pty and posts a
  readable event). It would look like "screen frozen until I press a key", not "typing shows
  late". Worth a test: one burst over 64 KB, a pause, then a marker line, no input.
- **Hook server reader slots** (unchanged from before): a connection that never sends holds
  one of the 16 slots on Windows; status updates can be dropped, nothing hangs.

## Next steps

1. **Shift test, no code needed.** The next time it lags, hold Shift and drag in the terminal:
   Shift bypasses mouse reporting, so the app draws that selection itself.
   - The Shift selection shows at once → the app paints fine; the delay is in the pty round
     trip, (a), (b) or (c).
   - The Shift selection is late too → (d), the main thread is busy.
2. **Same conversation in Windows Terminal** (`claude --resume`), fullscreen mode on. If it
   lags there as well, it is Claude Code or the machine, not the app.
3. **Timing log** (the app has no log file; Windows release builds have no stderr). Turned on
   by an environment variable, written to the config folder, one line per event with a
   timestamp:
   - input handed to the session (`Session::write`) and input written to the pty;
   - each `pty_read`: bytes, duration, whether it stopped at the 64 KB cap;
   - wakeup received on the main thread, and paint start/end;
   - any main-thread step over 50 ms.

   One lag with the log on gives the gap: write → first output byte is (b), output byte →
   paint is (c), key press → write is (a) or (d).
