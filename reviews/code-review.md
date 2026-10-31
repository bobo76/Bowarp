# Code review

Reviewed against the current worktree on 2026-10-24. This is a consolidated action list,
ordered by implementation priority, not vulnerability severity. Historical completed findings,
historical validation claims and low-value refactoring suggestions have been removed.

## High priority

### 1. Move last-command persistence off the UI thread

**Implemented by the high-priority agent, reviewed on 2026-10-24.** Previously,
`LastCommandChanged` called `save_tabs` on the UI thread, including JSON serialization
and file creation, writing and renaming. Changed, recordable commands and removal of
saved commands therefore blocked UI event handling on storage. Repeated identical and
trivial commands were already filtered; latency was not measured.

`TabsWriter` now queues snapshots immediately to one FIFO background writer, without a
debounce delay. GPUI entity access stays on the UI thread; serialization and file I/O do
not. Atomic replacement and permission handling are unchanged. Guests never start a
writer. Both close paths queue the final snapshot and drain the writer before ownership
is released, preventing older writes from overwriting newer state. Failures are reported.

Regressions cover ordered writes, HISTORY deletion, close/drop flushing, guest instances
and storage failures. The README now explicitly notes that a crash can lose queued or
in-flight updates, including deletions.

## Medium priority

No remaining findings.

## Low priority

### 2. Suppress duplicate session-kind text in hover cards

**Confirmed:** `WorkspaceView::tab_card` in
`crates\app\src\workspace_view\tab_card.rs` supplies the displayed tab name and
`launcher::default_name` independently. `crates\app\src\sidebar.rs` renders both without
comparison, so a default-named agent can show, for example, "Claude Code" twice.

Suppress the redundant kind label when it is equivalent to the displayed name, while
retaining the kind icon and genuinely different custom names. Cover default names and
custom names with tests.
