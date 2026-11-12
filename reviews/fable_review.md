# Review of Agent Workspace (whole workspace)

Date: 2026-11-09. Reviewed at `aa3dea1` (main, version 0.2.58), all crates, not a diff.

**Checks:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`
and `cargo test --workspace --locked` all pass on Windows (643 tests). The Unix-only tests and
`#[cfg(unix)]` code were not compiled or run.

None of the five problems below was reproduced; they come from reading the code.

## Findings

### Problem 1: [Major] `crates/remote/src/server.rs:322` - Closed remote sessions are never freed in the daemon

```rust
fanout.probe = Some(session.folder_probe());
```

`FolderProbe` holds the session's `Arc<Shared>`. `Shared` owns the event callback, and the
callback (lines 298-307) captures the same `Arc<Mutex<Fanout>>`. That is a reference cycle:
`Fanout → probe → Shared → callback → Fanout`. `close()` (379-399) only clears `clients`, so the
cycle survives.

- **Trigger:** every remote tab that is closed.
- **Impact:** the session's `Mirror` (a full terminal grid plus up to 10,000 lines of
  scrollback) and its `Shared` stay allocated until the daemon exits. The daemon only exits
  after 60 s with no session and no connection, so a long-running one grows with each closed
  tab.
- **Fix direction:** set `fanout.probe = None` in `close()` and in the `Exited` callback, or
  have the callback hold a `Weak` to the fanout.

### Problem 2: [Major] `crates/app/src/terminal_links.rs:146-162, 173-175` - Ctrl+hover stats untrusted paths on the UI thread

```rust
let path = links::resolve_path(&path_ref.path, folder, home)?;
if !path.is_file() {
```

`link_at` runs from `update_link_hover` and `refresh_link_hover` on the UI thread. The path text
comes from terminal output; `\` is a link character and absolute paths are accepted as they
are. The `file://` hyperlink branch does the same `is_file()`.

- **Trigger:** on Windows, a program prints `\\host\share\x.txt` (or an OSC 8
  `file:////host/share/x`) and the user holds Ctrl with the pointer over it. New output under a
  held Ctrl re-runs the lookup.
- **Impact:** the window freezes for the SMB timeout when the host is unreachable. Windows also
  attempts an authenticated SMB connection to a host chosen by whatever wrote the output. A
  stale NFS/sshfs mount freezes the same way on Linux.
- **Fix direction:** refuse UNC and network paths as path links in `resolve_path` (a pure
  function with `windows: bool`), and move the existence check to a background task.

### Problem 3: [Major] `crates/remote/src/server.rs:350-363` with `crates/remote/src/protocol.rs:18, 328` - A large snapshot makes a session impossible to re-attach

```rust
let snapshot = fanout.mirror.snapshot();
```

The snapshot goes out as one `Attached` frame with no size check, while the reader rejects any
frame over `MAX_FRAME` (16 MiB).

- **Trigger:** Reconnect to a session whose scrollback is dense with per-character truecolor
  (for example `lolcat` output or ANSI image art). By an estimate of about 24 bytes per cell,
  roughly 140 columns × 5,000 lines crosses 16 MiB.
- **Impact:** the client drops the whole host connection with "frame of N bytes", disconnecting
  every tab on that host, and each retry fails the same way. Above 32 MiB the server's own
  queue limit (`MAX_QUEUED_BYTES`) disconnects instead. The program keeps running but cannot be
  shown again.
- **Fix direction:** cap the snapshot (drop the oldest scrollback lines until it fits) or send
  it in chunks, and make `write_frame` refuse oversized frames.

### Problem 4: [Minor] `crates/app/src/file_explorer.rs:996-1002` - Directory symlinks and junctions cannot be deleted on Windows

`symlink_metadata(..).is_dir()` is false for a link, so the code calls `fs::remove_file`, which
Windows refuses for a directory link.

- **Impact:** the explorer lists it as a folder, the dialog promises to delete "the folder and
  everything in it", and the delete then fails with "permission denied".
- **Fix direction:** on Windows, use `remove_dir` when the link has the directory attribute;
  leave Unix as it is.

### Problem 5: [Minor] `crates/agent-status/src/ipc.rs:165-168` - `notification_type` goes onto the wire line unvalidated

Subagent ids and types pass through `token()`, but `notification_type` is pushed raw.

- **Impact:** a value with a space shifts the fields, and a second word such as `agent=x` would
  be parsed as a subagent id.
- **Fix direction:** pass it through `token()` like the others.

## Summary

- **Count:** 3 Major, 2 Minor.
- **Already tracked, not counted:** startup errors that only reach stderr are in `BACKLOG.md`
  (UX review #15). The same pattern exists for failed saves of `tabs.json`
  (`crates/app/src/tabs_file.rs:56, 58, 136, 146`) and the history file
  (`crates/app/src/history.rs:143, 151`) and for disabled shell integration
  (`crates/app/src/shell_integration.rs:246`); fold those into that item.
- **Coverage:** read in full: `agent-status`, `agentws-hook`, `remote`, and `term-core`'s
  session, prompt, mirror and snapshot code, plus persistence, links, quoting, hooks and
  startup. Not read line by line: `workspace_view.rs`, `terminal_view.rs`,
  `terminal_element.rs`, `sidebar.rs`, `settings_page.rs`, `history_popup.rs`, `launcher.rs`
  and most of the smaller `workspace-model` modules.

**Verdict:** Ready after fixing 1–3.
