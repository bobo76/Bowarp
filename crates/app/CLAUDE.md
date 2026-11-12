# UI thread and redraw rules (`app` crate)

GPUI runs rendering, layout, paint and every entity update on one UI thread. Anything slow
there delays the next terminal frame. These rules are for new code; the code that does not
follow them yet is listed in `reviews/performance-review.md` (Problems 6 and 7).

## Keep slow work off the UI thread

- No disk access, `git` or other process, or network call on the UI thread, even when it is
  "usually fast": a network share or a WSL path makes it slow. Path checks count
  (`is_file`, `exists`, `fs::canonicalize`), and so does parsing or laying out a large file.
- Pattern: `cx.spawn` the task, do the work inside `cx.background_spawn`, apply the result in
  `this.update`, and keep the `Task` in a field (`FileExplorer::read_listing`,
  `FileViewer::reload`). Use `cx.spawn_in` and `update_in` when applying needs the window.
- A newer request either replaces the stored task, which cancels the stale run, or waits for
  the running one and runs once more after it (`FileExplorer::check_changes`). Writes are
  never cancelled: the next one waits for the one running (`FileViewer::start_save`).

## Redraw only when needed

- Every `cx.notify()` redraws the whole window, terminal included. Only the file viewer is a
  cached view: it is redrawn when it (or an entity it shows) notifies or its size changes,
  so everything it draws must come from its own state.
- When applying a result, compare with the current state and notify or emit only if
  something visible changed (`FileExplorer::set_changes`).
- Don't notify per item in a loop. Coalesce bursts with a debounce or one run at a time.
- In prepaint or paint, notify only when something rendered earlier in the same frame is now
  out of date, as the search bar's count in `take_snapshot`. It costs a second frame.

## Per-frame terminal work

- Take the terminal lock once per frame (`take_snapshot`). Don't add another lock on the
  frame path: read what you need inside the snapshot. The prompt reads in shell tabs
  (`prompt_input` from `history_suggestion`, `restored_suggestion` and the history popup)
  are the existing exception.
- Work that scans the scrollback must be throttled while output streams, with a timer for
  the last refresh once the output stops (`OUTPUT_REBUILD_INTERVAL`, `refresh_search_after`).

## Tests

- Background work finishes only after `cx.run_until_parked()`.
- Work behind a timer (debounce, throttle) also needs `cx.executor().advance_clock(..)`
  first: `run_until_parked` does not advance the clock (`CHANGES_PAUSE`, `CHANGE_BATCH`,
  `refresh_search_after`).
