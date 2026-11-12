# Performance review (2026-11-12)

Two reviews merged: the whole-app performance review (hot paths: render, pty, wakeups,
background work, memory) and the UI redraw review (whether the explorer, the git page, the
file viewer and the tab cards can slow or freeze the redraw of the shell). Branch `git-page`
at `9faedce`. The findings fixed since are under "Fixed"; the open ones keep their numbers.

Method: code reading only. Nothing was measured or run, so the costs below are reasoned, not
profiled. No single defect explains a multi-second stall.

GPUI runs rendering, layout, paint and every entity update on one UI thread. Anything slow
there delays the next terminal frame. Background work (`cx.background_spawn`) does not, but
applying its result runs on the UI thread and usually asks for a redraw.

The rules for new code that came out of these reviews are in `crates/app/CLAUDE.md`.

## Open findings

### Problem 2: [Medium] the whole window redraws for any change

`crates/app/src/workspace_view.rs` (`impl Render for WorkspaceView`)

Any notify outside the file viewer redraws from the root. A terminal frame rebuilds the title
bar and every sidebar row with its listeners, and each explorer or git update repaints the
terminal too. The file viewer is now cached (see "Fixed"), so it is no longer rebuilt per
frame.

Fix direction: the sidebar is a method on `WorkspaceView` (`render_sidebar`), so caching it
means extracting an entity: do that only if the timing log shows it matters. BACKLOG.md
already has an item for caching the active `TerminalView`.

### Problem 6: [Low] small path checks on user actions

- Explorer "reveal": `fs::canonicalize` twice (`file_explorer.rs:272`).
- Markdown link click: `is_file` / `is_dir` / `exists` (`FileViewer::open_link`).
- Settings save: `settings_file::is_file` canonicalizes two paths (`settings_file.rs:201`).

One check each, on a click. Only slow on a network share.

Fix direction: leave as is unless network or WSL folders are common.

### Problem 7: [Low] up to seven terminal locks per frame in a shell tab

`crates/app/src/history_popup.rs:346-378`

One frame in a shell tab locks the terminal separately for each of these:

- `refresh_history` calls `prompt_input`, and `popup_placement` reads `display_offset`.
- `render_history_popup` calls `prompt_input` again.
- `take_snapshot` takes the lock for the grid copy.
- `history_suggestion` and `restored_suggestion` each call `prompt_input` in prepaint
  (`terminal_element.rs:250-256`); the first also calls the second.

During heavy output the I/O thread holds the terminal's data lock while it parses up to
64 KB (`try_lock_unfair` / `lock_unfair`, not the fair lock's lease), so each lock can wait
for one such parse. Each `prompt_input` also allocates a string per line tried in `locate`,
normally one, since the first line tried usually matches.

Only shell tabs pay this: `TerminalView::prompt_input` returns before locking when the tab is
not a shell (`history_popup.rs:160`), so a Claude tab takes no extra lock and this cannot
contribute to the Claude tab lag.

Fix direction: read the prompt input once inside `take_snapshot` and reuse it.

### Problem 8: [Low] one shaped line per non-ASCII cell

`crates/app/src/terminal_element.rs:863-886`

Only ASCII cells merge into batches. A 200-column rule of `─` becomes 200 `shape_line` calls
and 200 paints per frame, and agent TUIs draw many box characters.

Fix direction: merge runs of the same single-width non-ASCII character, or cache shaped
glyphs per character and style. Measure first. Merging runs is risky: a fallback font's
advance may not match the cell width, which is why the code comment excludes these cells.
Not checked: whether GPUI's line cache already absorbs the repeated `shape_line` calls.

### Problem 9: [Low] scrollback is fixed at 10,000 lines

`crates/term-core/src/session.rs:413-417`

`term::Config::default()` is used and there is no setting. At roughly 24 bytes per cell, a
full history at 200 columns is about 48 MB per tab, so about 380 MB for eight wide tabs. The
figure is a worst case, and the 24 bytes per cell was not verified.

Fix direction: a scrollback setting (E2 in `ux-review.md`), possibly with a lower default for
agent tabs that use the alternate screen. Low value unless memory use is a complaint.

## Fixed

After the first review:

- **Search rescan on every output frame:** the rebuild is throttled while output streams
  (`OUTPUT_REBUILD_INTERVAL`, 200 ms), with a last refresh once it stops
  (`refresh_search_after`).
- **File system reads on the UI thread:** the tab's folder and branch (`Location::read`) and
  the explorer's folder listings are read in the background.
- **Explorer change colors ran the full git line count:** the marks now come from
  `git status` only.

After the merged review:

- **Problems 1 and 3, the viewer read files on the UI thread:** opening a file and reloading
  it after a change both read it in the background (`FileViewer::reload`, applied by
  `show_read`); the panel opens empty until the first read ends. Showing the text (the
  editor's `set_value`, the JSON layout, the SVG drawing) still runs on the UI thread, and
  gpui-kit's `TextView` parses the Markdown itself.
- **Problem 4, saving read and wrote on the UI thread:** the check for a change on disk and
  the write run in the background (`start_save`, `write_file`); a save waits for the one
  before it, and edits typed while it writes stay unsaved.
- **Problem 5, the folder watcher resolved paths on the UI thread:** the explorer looks up a
  folder's real path in the background before reading it and hands it to
  `WatchedFolders::set_real_path`.
- **Problem 2, viewer half:** the file viewer is a cached view, redrawn only when it
  notifies or its size changes. Not checked on screen yet.
- **Problem 10, no `[profile.release]`:** `lto = "thin"` and `codegen-units = 1` are set.
  The gain was not measured.

## What looked fine

- **Idle:** there are no polling timers; the spinner clock only ticks after a frame that
  drew a spinner.
- **Hidden tabs:** they acknowledge wakeups without painting.
- **Lists:** the explorer and the diff lists are virtualized.
- **Off the UI thread:**
  - Explorer: folder listings, `git check-ignore`, change colors, deleting.
  - Git page: status, diffs, staging, committing, finding repositories, `watched_folders`.
  - Tab cards: line counts (`working_changes`).
  - Tabs: folder and branch, Ctrl-hovered link lookups, tab saves, history writes.

## Summary

- Open: 1 Medium (2, sidebar half), 4 Low (6 to 9), no correctness defects.
- Measure first: the sidebar half of 2, and 8.
- Low value: 6, 7 and 9.
- Relation to `claude-tab-lag.md`: Problem 2 is the remaining candidate for cause (d), a
  busy main thread, in that note. Build the timing log from the note's step 3 before the
  sidebar half of Problem 2. Problem 7 does not apply to Claude tabs.
- Not done: nothing was profiled, before or after the fixes.
