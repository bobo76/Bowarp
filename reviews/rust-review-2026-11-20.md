# Rust review — whole workspace (2026-11-20)

Scope: all 6 crates (~71k lines), branch `backlog` at 3a32d8d. Review only; no code changed.
Severity scale: Critical / High / Medium / Low.

**Verdict: Ready with fixes 1–6.** Toolchain fully green; nothing blocks the build, but there is
one High security issue in the remote daemon and several Medium data-loss / freeze risks.

## Toolchain

| Check | Result |
|---|---|
| `cargo fmt --check` | PASSED |
| `cargo clippy --workspace --all-targets --all-features --locked -D warnings` | PASSED, 0 warnings |
| `cargo test --workspace --all-targets` (debug) | PASSED, ~1,005 tests, 0 failed |
| `cargo test --doc` | PASSED (no doctests) |
| `cargo deny check` | advisories / bans / licenses / sources ok |
| `cargo audit` | not installed |

Context: edition 2024, MSRV 1.95, `alacritty_terminal =0.26.0`. Every finding was re-checked
against the code (see `rust-review-2026-11-20-verification.md`); wrong or overstated claims were
removed and fixes corrected. Medium 2 was checked against alacritty 0.26.0 with a scratch program.

## High

### 1. The remote proxy trusts whatever listens on the daemon's socket name
- Location: `crates/remote/src/transport.rs:111-122`
- Code:
  ```rust
  pub fn connect(name: &str) -> io::Result<Stream> {
      Stream::connect(name.to_ns_name::<GenericNamespaced>()?)
  }
  ```
- Only the daemon checks the peer (`from_same_user`, line 98); the proxy never does
  (`connect`, `connect_or_start`, `start_daemon`, `wait_until_stopped`).
- On Linux, `agentws-server-<uid>.sock` is in the abstract namespace, which has no file
  permissions. Another user on a shared host can bind that name. No race is needed: the daemon
  exits after 60 s idle, the name is then free, and the proxy's retry loop connects to whoever
  holds it.
- That user then receives every `Spawn` (environment, tokens) and every keystroke, sudo
  passwords included, and can send fake output back.
- Fix: in `connect` on Unix, check `stream.peer_creds()?.euid() == Some(geteuid())` and fail
  with a clear "socket owned by another user" error.
- Windows (Low): the default pipe DACL gives other users read access only, so they can't open it
  read/write, and interprocess already sets `FILE_FLAG_FIRST_PIPE_INSTANCE`. Only squatting the
  machine-wide name remains; check the server's owner via `GetNamedPipeServerProcessId`. Reword
  the comment at lines 104-105 and docs/remote.md:22 ("the default DACL", not "a security
  descriptor").

## Medium

### 2. A bad terminal size from the wire can take down the whole daemon
- Location: `crates/remote/src/server.rs:523, 578-583, 686-690`; decoder `protocol.rs:602`
- `TermDimensions` is four raw `u16`s, never clamped. Checked against alacritty 0.26.0: a
  `Spawn` with 0 columns **segfaults** in a release build (`Term::new`; debug asserts), 0
  lines or a resize to 0 columns/lines panics, and 65535×65535 aborts on allocation. A segfault
  or abort takes down the whole daemon and every session in it.
- After a panic in a connection thread, `connections` is never decremented (`fetch_add` at
  `server.rs:282`, `fetch_sub` at 303, no drop guard), so the daemon never idles out.
- Only same-user peers reach the daemon and the app clamps on its side
  (`terminal_element.rs:549-556`), so this needs a buggy or third-party client.
- Fix: clamp in one place (`Decoder::size`, or term-core's `Session` spawn/resize and
  `Mirror`) to about columns 2..=1000, lines 1..=500. `1..=4096` is too loose: two grids plus
  scrollback at 4096×4096 is hundreds of MB. Make the connection counter a drop guard.

### 3. A silent remote can stall connecting while holding the host's lock
- Location: `crates/remote/src/client.rs:142-146, 176`; `crates/app/src/remote_hosts.rs:223`
- The handshake `read_frame` has no timeout. `ConnectTimeout` and `ServerAliveInterval`
  bound network hangs, but a remote that is alive and silent (a wedged daemon, an rc file reading
  stdin) blocks it indefinitely.
- On the error path, the stderr `join()` can wait while a freshly forked `ControlPersist` master
  still holds ssh's stderr, up to the ControlPersist timeout.
- The per-host mutex is held throughout (deliberately), so every later tab or Reconnect on that
  host waits with no message.
- Fix: a watchdog that kills the ssh child after about 30 s (this unblocks both `read_frame` and
  the stderr join); optionally bound the stderr wait like `status.rs`'s `OUTPUT_GRACE`.

### 4. "Discard all" can delete files the user never saw in the confirmation
- Location: `crates/app/src/git_page.rs:656-661` → `crates/workspace-model/src/git/actions.rs:175-183`
- The dialog shows counts from the current status ("N files … M files not tracked by git are
  deleted", `discard_prompt`, `actions.rs:220-241`), but the action runs `git restore :/` and
  `git clean -f :/` on the whole tree. It also deletes untracked files an agent creates while
  the dialog is open, which are in no commit and no index, and files left out when the status
  was truncated (`MAX_STATUS_FILES`).
- Fix: keep `restore :/` for tracked files (it only touches files the user modified). Run
  `clean` only on the untracked paths that were counted, in chunks (thousands of paths in one
  command line exceed Windows' ~32K limit), with `--literal-pathspecs`. Refuse "Discard all"
  while the status is truncated.

### 5. Git write commands killed at their timeout leave `.git/index.lock` behind
- Location: `crates/workspace-model/src/git/runner.rs:124-127`
- Commit gets 300 s; stage, unstage, apply and discard 30 s; then SIGKILL. A slow pre-commit
  hook or LFS filter leaves a stale `index.lock`, and every later git command fails, the
  user's own included.
- Fix: don't kill write commands, like branch switching already does (`branches.rs:14`).
  Optionally send SIGTERM before SIGKILL on Unix so git removes its own lock. Never auto-delete
  `index.lock`: it may not be ours.

### 6. Opening a FIFO or device file reads forever or runs out of memory
- Location: `crates/app/src/file_viewer.rs:1271-1275`
- `read_text` checks the metadata size (0 for a FIFO or device), then `fs::read`, with no
  `is_file()` check. A FIFO blocks a background thread forever; `/dev/zero` allocates until OOM.
- The explorer lists FIFOs and devices as files, and `metadata` follows symlinks: a repo
  holding `notes.txt -> /dev/zero` crashes the app on one click.
- Fix: check `file_type().is_file()` and read with `File::open(..).take(MAX + 1)`; same check
  in `check_image`.

### 7. `file://host/...` links get past the network-path guard
- Location: `crates/workspace-model/src/links.rs:313-318`, used by `crates/app/src/terminal_links.rs:233-243`
- For `file://evil-host/share/x.exe`, `rest` has no leading `//`, so `is_network_path` is
  false and the path is treated as relative. When it doesn't exist, it falls back to
  `Unchecked::Url(uri)` and the OS opener gets it: on Windows, SMB to a host chosen by program
  output (NTLM leak).
- Also, `hyperlink_target` (`terminal_links.rs:240-243`) sends **every** `file:` URL that
  `local_link` rejects to `Unchecked::Url`. So `file:////host/share` and `file://///host/share`,
  which the guard does catch on Windows, still reach the opener: fixing the host parsing alone
  doesn't close this.
- `strip_prefix("localhost")` also turns `file://localhostfoo/x` into `foo/x`.
- Fix: parse the host properly and accept only an empty host or exactly `localhost` followed by
  `/`; in `hyperlink_target`, return `None` for a `file:` URL that `local_link` rejects instead
  of falling back to `Url`. Test `file://host/x`, `file:////host/x`, `file://localhostfoo/x`.

### 8. Markdown links in the file viewer send every non-local URL to `cx.open_url`
- Location: `crates/app/src/file_viewer.rs:719-735`
- Includes `ms-msdt:`, `search-ms:`, `smb:` and UNC paths: one click on a README link in an
  untrusted repo triggers them.
- The terminal already has an allowlist (`HYPERLINK_SCHEMES`, `links.rs:14`); the viewer
  doesn't use it.
- Fix: allow only `http`, `https` and `mailto`, through one helper shared with
  `terminal_links::hyperlink_target`; rejected `file:` URLs and network paths get a "Can't
  open" notification.

### 9. One unknown tab kind or color in `tabs.json` loses every saved tab
- Location: `crates/workspace-model/src/tabs_state.rs:39-41, 155-157`
- After running an older build on a file from a newer one, the parse fails, the app starts
  with a fresh tab, and the next save overwrites the file. An unknown group color fails it
  too (`layout.rs:102`). Settings have a lenient loader for this; tabs don't.
- Fix: parse tabs one by one and drop only the ones that fail; unknown colors become `None`.
  Optionally move an unparsable file to `tabs.json.bak` before the first save.

### 10. Tests read the real `settings.json`, against CLAUDE.md
- `settings_file::init(cx)` (which calls `config_file("settings.json")`) is used at:
  - `crates/app/src/workspace_view/tests.rs:46, 1022, 1121, 1161, 1801, 2802, 2889`
  - `crates/app/src/workspace_view/settings_pane/tests.rs:19`
  - `crates/app/src/workspace_view/tab_card/tests.rs:17`
  - `crates/app/src/history_popup/tests.rs:461`
  - `crates/app/src/terminal_view/search/tests.rs:22`
- Results depend on the developer's settings (e.g. `history_popup/tests.rs:461` fails with the
  history popup turned off). None of these tests reaches `update` today, but a future one would
  write the real file. Fix: use `init_in_memory` (already used by ~30 tests); add a variant that
  calls `install` if a test needs the theme observer.

### 11. Editing or removing a remote host uses a row index on a re-read list
- Location: `crates/app/src/settings_page/remote_hosts.rs:126, 208-211`
- `update` re-reads the file first; if another instance or a hand edit reordered hosts, the
  action hits the wrong host. Fix: find the host by value inside the closure.

### 12. "Clear history" blocks the UI thread
- Location: `crates/app/src/history.rs:114-120`
- `recv()` waits on the worker, which waits on a file lock with no timeout. Normally
  milliseconds (other holders are short appends); noticeable only with the config folder on a
  slow or hung network filesystem. Fix: await the reply from a task.

### 13. Pastes over 16 MiB silently stop the remote client's writer
- Location: `crates/remote/src/client.rs:533-541, 365-367`
- `write_frame` refuses the frame and the writer thread just returns while `connected` stays
  true; typing and pings are dropped and the tab disconnects ~45 s later.
- Fix: split input under `MAX_FRAME`; log and mark disconnected when a write fails.

### 14. A tab can stay "Active" for good
- Location: `crates/app/src/terminal_view.rs:1195-1199`
- If `last_output()` is `None` (a resize wakeup before any output), the task returns without
  clearing `activity_task`; `tab_status` (line 686) reports Active forever and
  `track_activity` never runs again.
- Only remote tabs: the only wakeup before any output is `SessionFeed::resized`, when another
  client resizes the session. Fix: let `None` fall through to the clearing code.

### 15. The scroll bar's press handler is window-wide and only checks geometry
- Location: `crates/app/src/terminal_element.rs:410-437`, `crates/app/src/terminal_view.rs:1353`
- `.occlude()` doesn't stop window-wide listeners. Menu items stop propagation, but a click on a
  dialog backdrop, menu padding or separator, or a toast over the right edge also starts a thumb
  drag.
- Fix: require `hitbox.is_hovered(window)` on mouse down; keep move and up window-wide.

### 16. Snapshots of a terminal in origin mode put the cursor at the top-left
- Location: `crates/term-core/src/snapshot.rs:79-94`
- CUP is written before `?6h`, and alacritty homes the cursor when DECOM is set
  (`term/mod.rs:1982-1985`), so a remote client attaching gets its cursor at 1;1.
- The snapshot also never restores the scroll region (DECSTBM). In origin mode, cursor moves are
  relative to the region top, so later moves land wrong until the app sets the region again.
- Fix: write DECSTBM, then `?6h`, then the CUP with its row relative to the region top when
  origin mode is on. Only moving `?6h` before the CUP is correct only for a full-screen region.

### 17. Disk work on the UI thread (`crates/app/CLAUDE.md`)
All small local syscalls; slow only on network or hung mounts.
- `crates/app/src/hooks.rs:204-235`: plugin folder and files written on every tab start
  (tmpfs), `remove_dir_all` on every close; moving the remove to the background is easy.
- `crates/app/src/file_explorer.rs:636-645` and `:1138-1148` (rename, create); trash and
  delete already use `background_spawn`.
- Already accepted in `reviews/performance-review.md` Problem 6: `file_explorer.rs:306`
  (`canonicalize`, listed there under the stale line 272) and `file_viewer.rs:720-724`.
- `crates/app/src/terminal_view.rs:610, 1486` (hidden-cd file write and delete).
- `crates/workspace-model/src/tabs_state.rs:70-83` (`is_dir` up every ancestor on restore).

## Low

| Location | Issue |
|---|---|
| `crates/remote/src/protocol.rs:595` | `with_capacity(count.min(len))` reserves `size_of::<T>()` bytes per item: a bogus count can reserve >1 GB; the "cannot reserve much" comment is wrong. Cap at `remaining / 4` |
| `crates/remote/src/server.rs:578-583, 686-690` | Pty and mirror resizes are separate critical sections; two clients can leave them at different sizes. Fix with a per-session resize lock, not the fan-out lock: alacritty's `pty_read` holds the term lock while the tap takes the fan-out lock, so resizing under it deadlocks |
| `crates/remote/src/transport.rs:87-91` | Idle check can run between accept and `connections += 1`: daemon exits on a client that just connected |
| `crates/app/src/remote_hosts.rs:223` | `lock().unwrap()` on a poisoned mutex would leave the host unusable until restart (nothing on the connect path panics today); use `unwrap_or_else(PoisonError::into_inner)` |
| `crates/agent-status/src/ipc.rs:331-352` | Accept loop busy-spins on EMFILE; a panic in `thread::spawn` kills the hook server |
| `crates/agent-status/src/ipc.rs:159-163, 303-305` | `hook_event_name` skips `token()`; a line cut at `MAX_LINE` is still parsed (same-user peer: at worst a wrong status) |
| `crates/agent-status/src/ipc.rs:130-131`, `crates/app/src/hooks.rs:267` | Socket and `/tmp` names predictable from the pid: another user can squat them and disable agent status (no interception). Also, `create_private_dir` (`hooks.rs:357-367`) accepts an existing dir without checking its owner |
| `crates/term-core/src/session.rs:421-440` | If spawn fails after the pty exists, the pty drops on the caller's (UI) thread, which waits for the child with no limit |
| `crates/app/src/terminal_view/resume.rs:54` | A conversation id starting with `-` is later passed as a separate argument after `--resume` and read as a flag; pass `--resume=<id>` or validate the id |
| `crates/app/src/terminal_view/pointer.rs:122-125` | With chorded presses (hold one button, press another), the first button's release is never reported; only SGR mode loses which button was released |
| `crates/app/src/terminal_links.rs:211-217` | Wide-char spacer cells read as spaces: paths with CJK or emoji aren't detected as links |
| `crates/app/src/git_page.rs:1077, 1090` | Stage/Unstage use a row index captured at render time, which can name another file after a refresh |
| `crates/workspace-model/src/git/actions.rs:92-97` | `trim_end` removes trailing spaces from the repo path (`repo_top` does it right) |
| `crates/workspace-model/src/git/status.rs:225` | Non-UTF-8 paths converted lossily, then used as pathspecs |
| `crates/workspace-model/src/git/runner.rs:141` | Comment says repo config can't run commands, but `filter.*` drivers still run |
| `crates/workspace-model/src/git/runner.rs:197-207` | Output reader threads leak when a hook's child keeps the pipes open |
| `crates/workspace-model/src/git.rs:134-152` | `marks` marks every ancestor up to `/`, against its doc |
| `crates/workspace-model/src/file_tree.rs:415-426` | Paths with backslashes quoted wrongly for fish |
| `crates/workspace-model/src/window_state.rs:155-187`, `history.rs:395-406` | Atomic writes have no `sync_all`; temp file left behind on failure |
| `crates/workspace-model/src/instance_lock.rs:15` | An empty lock file (still being written) counts as stale: two instances can both take the lock |
| `crates/workspace-model/src/tabs_state.rs:151` | `to_json` panics on a non-UTF-8 folder (the app guards this today) |
| `crates/workspace-model/src/tab_history.rs:75-83` | `remove` can leave Back pointing at the selected tab |
| `crates/workspace-model/src/agents/claude.rs:141` | Windows hook command quoted with bare `"…"`: `$` in the path expands |
| `crates/app/src/hooks.rs:324-341` | `?` on one entry stops the whole stale-file cleanup |
| `crates/app/src/window_background.rs:264-277` | A failed Windows backdrop is retried, and its warning re-shown, on every settings change |
| `crates/app/shell-integration/zsh/.zshenv:99-104` | Unquoted `$dir` / `$file`: with `sh_word_split` or `glob_subst`, paths with spaces break. Quote them; don't add `emulate -L zsh`, which would also run the user's precmd and prompt theme with default options |
| `crates/app/src/shell_integration.rs:269-276` | Scripts rewritten in place, not atomically: another instance can source a half-written file |
| `crates/app/src/folder_watcher.rs:50-59` | Unbounded channel that keeps duplicate paths |
| `crates/app/src/settings_page.rs:909` | `current_exe()` runs on every render; cache it like `VERSION` |

## Top 3

1. Proxy socket ownership check (High 1): keystrokes and environment exposed to other users on
   shared hosts.
2. "Discard all" and `.git/index.lock` (Medium 4, 5): both can destroy or block the user's work.
3. Bad sizes and oversized frames on the remote protocol (Medium 2, 13): one bad message takes
   down a daemon or a connection.

## Good patterns worth keeping

- Small `unsafe` blocks with accurate `SAFETY` comments (`kill_child`, `MarkedPty::register`,
  `renameat2`).
- Every git path argument goes through `--literal-pathspecs` and `--`, with deadlines and
  output read while git runs.
- Settings are read-modify-write with lenient fields; remote frames are size-capped, with
  snapshots fitted under `MAX_FRAME`.
- OSC 133 and OSC 9;4 parsers are resumable byte state machines with bounded payloads.
