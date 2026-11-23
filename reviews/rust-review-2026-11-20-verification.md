# Verification of rust-review-2026-11-20.md

Each claim was checked against the code at 3a32d8d (read-only). All cited line numbers still
match. To check Medium 2, a scratch program was run against `alacritty_terminal` 0.26.0.
Verdict: **C** = confirmed, **P** = partly true, **F** = false. Worth doing: **YES / LOW / NO**;
effort S/M/L.

**Summary:** no claim is entirely false. Two sub-claims are false: Medium 2's mutex-poisoning
part, and the Windows half of High 1. Several are overstated. The review also missed three
issues: the `file:` URL fallback to the opener (Medium 7), the scroll region left out of
snapshots (Medium 16), and the release-build segfault (Medium 2).

## Do these (real impact, small effort)

| # | Item | Verdict | Effort | Notes / better fix |
|---|---|---|---|---|
| H1 | Proxy trusts socket owner | C on Linux, P on Windows | S | Linux: a squatter needs no race, because the daemon idles out after 60 s and the name is then free. Fix: `peer_creds().euid()` check in `connect`. Windows: the default DACL already gives other users read access only, and interprocess already sets `FIRST_PIPE_INSTANCE`; only name squatting remains (LOW, M). Reword the comment at transport.rs:104-105. |
| M2 | Unclamped TermDimensions | P | S | Worse than stated: 0 columns **segfaults** in release (`Term::new`), and a huge size aborts on allocation. The **mutex-poisoning claim is false**: the session resize panics before the fan-out lock is taken, and the term lock is a `FairMutex` that doesn't poison. The connection-count leak is true. Clamp tighter than 1..=4096 (e.g. columns 2..=1000, lines 1..=500), and make the counter a drop guard. |
| M4 | "Discard all" covers the whole tree | P | S-M | The dialog shows *counts*, not file names, but files an agent creates meanwhile are deleted by `git clean`. Don't pass every path to `Discard(files)`: it can exceed the Windows command-line limit. Keep `restore :/`, run `clean` only on the counted untracked paths (in chunks), and block while the status is truncated. |
| M5 | Killed git write leaves index.lock | C | S | Plausible with a long pre-commit hook or a GPG pinentry left unanswered. Don't kill write ops (optionally SIGTERM first on Unix). Never auto-delete the lock. |
| M6 | FIFO or device in file viewer | C | S | A repo symlink `x.txt -> /dev/zero` crashes the app (OOM) on one explorer click. Add an `is_file()` check and `take(MAX+1)`; check the image path too. |
| M7 | `file://host` gets past the guard | C, and broader | S | Missed by the review: `hyperlink_target` (terminal_links.rs:240-243) sends **every** `file:` URL that `local_link` rejects to the opener, `file:////host` included. Fix the host parsing **and** return `None` instead of falling back to `Url`. |
| M8 | Markdown links open any scheme | C | S | The terminal already has `HYPERLINK_SCHEMES`; the viewer doesn't use it. Share one allowlist helper. |
| M9 | One bad entry loses all tabs | C | S-M | Unknown group colors fail the parse too. Parse entries one by one; optionally keep `tabs.json.bak` like settings does. |
| M10 | Tests read the real settings.json | C (write risk overstated) | S | All 11 call sites confirmed. None reaches `update` today, so the tests read the file but don't write it. Switch them to `init_in_memory` (already used in ~30 tests). |
| M13 | Paste >16 MiB kills the remote writer | C | S | Chunk `Input` (e.g. 1 MiB) and mark the client disconnected when the writer fails. |
| M14 | Tab stuck "Active" | C (remote tabs only) | S | Only a resize from another client before the first output triggers it. One-line fix in the `else`. |
| M15 | Scroll bar press is window-wide | P | S | Menu items stop propagation, so no drag from them; dialog backdrops, menu padding and toasts do trigger it. Check `hitbox.is_hovered` on mouse down. |
| M16 | Origin-mode snapshot cursor | C, and broader | S | The snapshot also never writes DECSTBM (scroll region). Write the region, then `?6h`, then the CUP relative to the region. |
| L | tab_history `remove` leaves Back = current | C | S | Real dead key press after closing a background tab; add a unit test. |
| L | resume.rs `--resume` with an id starting with `-` | C | S | Not privilege escalation (peer is the same user). Use `--resume=<id>` or validate the id's shape. |
| L | zsh `__agentws_cd` | C | S | **Use a different fix:** quote `"$dir"` and `"$file"`. `emulate -L zsh` would also apply to the user's precmd and prompt themes and break them. |

## Low priority (true, but rare or cosmetic; do when nearby)

| Item | Verdict | Note |
|---|---|---|
| M3 remote connect hang under lock | P | Lock held: true. "Forever": overstated, since ConnectTimeout and ServerAlive bound network hangs. A 30 s watchdog kill fixes both. |
| M11 remote host edit by index | C | Needs another instance to reorder hosts mid-click. Find the host by value. |
| M12 "Clear history" blocks the UI | C | Milliseconds unless the config folder is on a hung network FS. |
| M17 file_explorer rename/create | C | Not in performance-review.md: add it to Problem 6, or move to `background_spawn` like trash. |
| M17 hooks.rs plugin write and remove | C | tmpfs writes; pty spawn on the same path costs more. Moving remove-on-close to the background is easy. |
| M17 terminal_view.rs:610,1486 | C | Tiny files. Add to performance-review.md Problem 6. |
| M17 tabs_state.rs:70-83 `is_dir` | C | One stat per tab at startup. Only matters with dead network mounts. |
| L protocol.rs:595 `with_capacity` | C | Comment is wrong; cap at `remaining/4`. |
| L server.rs pty/mirror resize race | C | **Don't** move the resize under the fan-out lock (deadlocks with alacritty's `pty_read`); use a separate per-session resize lock. |
| L remote_hosts.rs:223 poisoned lock | C | Nothing panics there today; `unwrap_or_else(PoisonError::into_inner)`. |
| L ipc.rs accept busy-spin and `thread::spawn` panic | C | Sleep on accept error; `thread::Builder`. |
| L ipc.rs `hook_event_name` / cut lines | C | Impact overstated (same-user peer). One-line `token()`; drop lines without a newline. |
| L predictable socket and /tmp names | P | Squatting disables agent status only; no interception. Better hardening: check the owner of an existing dir in `create_private_dir`. |
| L claude.rs:141 Windows hook quoting | C | Matters only if hooks run under bash; check that first. |
| L shell_integration non-atomic rewrite | C | Write to a temp file, then rename. |
| L terminal_links wide-char spacers | C | Real gap for CJK users; effort M (cell-to-char offset mapping). |
| L git_page row index | C | Window is about one frame, and Discard confirms by name. |
| L actions.rs `trim_end` | C | One-line `trim_end_matches('\r')`. |
| L runner.rs:141 comment | C | Comment fix only; filters must keep running (LFS). |
| L git.rs `marks` | C | No visible effect (caller filters). Fix the doc or the code. |
| L fish backslash quoting | C | Needs a fish variant of `ShellSyntax`. |
| L atomic writes without `sync_all` | C | Cheap; share one helper. |
| L instance_lock empty file | C | Microsecond window; hard-link a temp file, or use an mtime grace period. |
| L tabs_state `to_json` `expect` | C | Guarded today; map to `io::Error`. |
| L pointer.rs second button | P | Only chorded presses; only SGR loses the button identity. |
| L window_background retry | C | Windows only; the toast says "restart to retry" but the app retries itself. |
| L hooks.rs:324-341 `?` in sweep | C | Use `continue` per entry. |
| L settings_page `current_exe` | C | A readlink, not disk work; a `LazyLock` for consistency. |

## Not worth doing

| Item | Why |
|---|---|
| M17 file_viewer.rs:720-724, file_explorer.rs:306 | Already in performance-review.md Problem 6 as accepted (that line number is stale: 272 → 306). |
| M17 settings_pane.rs:81 `reload` | Every settings toggle already does the same read-modify-write; moving one call buys nothing. |
| L status.rs:225 lossy paths | Fails safely (literal pathspec matches nothing, git reports an error). |
| L runner.rs reader-thread leak | Bounded, rare, deliberate trade-off. |
| L transport.rs idle race | Microsecond window; the next try starts a new daemon. |
| L session.rs pty drop on spawn failure | Only on fd exhaustion; the fix needs a `Drop` wrapper. |
| L folder_watcher duplicates | All consumers dedupe or only test membership. |
| Toolchain: `cargo audit` | Confirmed not installed, but `cargo deny` already covers RustSec. Test count ≈1,008 `#[test]` fns, consistent with the review. |
