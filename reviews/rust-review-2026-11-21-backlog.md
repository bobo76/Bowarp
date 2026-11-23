# Rust review — branch `backlog` vs `main` (2026-11-21)

Scope: 47 commits since `main` (merge base 4bfd48b), 67 Rust files, +3,436 / −339, at
43110e7. Most commits fix items of `rust-review-2026-11-20.md`; the branch also adds Git page
branch switching. Review only; no code changed. Severity scale: Critical / High / Medium / Low.

**Verdict: ready once H1–H3 are fixed.** H1 and H3 are new problems caused by this branch's
fixes, and H2 is a race this branch introduced. The Medium and Low items don't block. The
previous review's other fixes check out.

## Toolchain

| Check | Result |
|---|---|
| `cargo fmt --all --check` | PASSED |
| `cargo clippy --workspace --all-targets --all-features --locked -D warnings` | PASSED, 0 warnings |
| `cargo test --workspace --all-targets --locked` (debug) | PASSED, 0 failed |
| `cargo deny check` | advisories / bans / licenses / sources ok |
| `cargo audit` | not installed |

Context: edition 2024, MSRV 1.95, `alacritty_terminal =0.26.0`. Each High finding was re-checked
against the code at 43110e7.

## High

### H1. Editing one field of a remote host, then another, silently loses the second edit
- Location: `crates/app/src/settings_page/remote_hosts.rs:208-217`, with
  `crates/app/src/settings_page.rs:1214-1240` (`text_field`)
- Code:
  ```rust
  let host = host.clone();
  let set = Rc::new(move |settings, value| {
      settings.remote_hosts.set_field(index, &host, field, value);
  });
  ...
  key: format!("remote-host-{index}-{field:?}").into(),
  ```
- Cause: `text_field` copies the binding only once, in the `use_keyed_state` setup closure.
  Its save and release handlers keep that copy. The state key doesn't include the host's
  values, so the `host` snapshot stays as it was on the field's first render.
- Failure:
  1. Add a host.
  2. Type `me@box` in Destination and press Tab. This saves.
  3. Type `Box` in Name. The Name binding still holds the host with an empty destination,
     so `locate` matches no host and `set_field` does nothing. The name is lost with no
     message.
- Regression: the old index-based code handled this flow. The model test
  `edits_follow_the_shown_host_when_the_list_was_reordered` passes only because it builds a
  fresh `shown` for each call.
- Fix: put the other fields' values (or a hash of them) into the binding key, or have the
  binding read the current host through an `Rc<RefCell<RemoteHost>>` refreshed on each
  render. Add an app test that edits Destination and then Name.

### H2. A failed tab start reuses its session id; the plugin folder is removed on a detached thread
- Location: `crates/app/src/hooks.rs:238-248`, `crates/app/src/workspace_view.rs:681-715`
- Code:
  ```rust
  /// ... Session ids are never reused, so nothing waits for it.
  pub fn remove_session(&self, session: u64) -> thread::JoinHandle<()> {
      ...
      thread::spawn(move || plugins.iter().for_each(|plugin| remove_artifact(plugin)))
  }
  ```
  ```rust
  let view = TerminalView::spawn(session_id, &launch, reporting, shell_key, cx).inspect_err(
      |_| { if let Some(hooks) = &self.hooks { hooks.remove_session(session_id); } },
  )?;
  ...
  self.next_session_id += 1;   // skipped by the `?` above
  ```
- Cause: the doc comment's premise is false. When `TerminalView::spawn` fails,
  `start_session` returns before `next_session_id += 1`, so the next tab gets the same id.
- Failure: during restore (`workspace_view.rs` `restore`), one tab fails to start (for
  example, its agent was uninstalled). The next tab attaches with the same id N and writes
  `<plugin_root>/N/hooks.json`, while the detached thread may still be running
  `remove_dir_all(<plugin_root>/N)`. Copilot then starts with a missing `--plugin-dir` and
  never shows Waiting. Before this branch the removal was synchronous, so this race is new.
- Fix: increment `next_session_id` at the top of `start_session`, before anything can fail
  (check `spawn_tab` and `reconnect.rs` too), and correct the doc comment. Add a test: a
  failed spawn followed by a successful one keeps the second tab's plugin folder.

### H3. `file://` links that carry the local hostname are no longer links
- Location: `crates/workspace-model/src/links.rs:332-335`, used by
  `crates/app/src/terminal_links.rs:243-251` (`hyperlink_target`)
- Code:
  ```rust
  let host = &rest[..rest.find('/').unwrap_or(rest.len())];
  if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
      return None;
  }
  ```
- Failure: the OSC 8 spec says to put the machine's hostname in `file://` URLs, and
  `ls --hyperlink=auto` does so (`file://mybox/home/u/x.rs`). These links now get `None` and
  can't be clicked. Before, they opened through the OS opener.
- Windows: `file://C:/x`, which some Windows tools emit, is now refused too, because `C:` is
  read as a host. Before, it opened as `C:/x`.
- Fix: also accept the local hostname (read once) and, on Windows, a drive-letter "host".
  Add tests for both.

## Medium

### M1. The delayed hidden `cd` doesn't check again that the shell is still at its prompt
- Location: `crates/app/src/terminal_view.rs:617-626`
- Failure: the folder write now runs off the UI thread. If the user starts a command (`vim`, an
  agent) before it finishes, `HIDDEN_CD_KEYS` is sent into that program. If the write failed,
  `cd '/x'\r` is typed into it instead.
- Fix: in the `update` closure, return early unless `view.session.prompt_input().is_some()`.

### M2. Gaps in test coverage
- Picker test passes even without the check it claims to cover:
  `crates/app/src/git_page/branch_picker/tests.rs:157-162` calls
  `pick_branch(..., SwitchPlan::Nothing)` to check that a dirty status blocks a switch.
  `Nothing` never emits an event anyway, so deleting `switch_blocker()` wouldn't fail the
  test. Use `SwitchPlan::Run(...)` and assert that a notification was pushed.
- No test for the failed-spawn path behind H2.
- No test for the hidden `cd` in M1 (no `change_dir`/`cd_file` in
  `terminal_view/tests.rs`).
- `chorded_presses_report_every_release` doesn't check a drag between the two releases, which
  is what the `reported_buttons.last()` logic in `pointer.rs:170-175` handles.

### M3. The `cd` file can be left behind when a tab closes
- Location: `crates/app/src/terminal_view.rs:1505-1506`
- Cause: the `remove_file` thread can run before an `fs::write` that is already running in
  the background; the file stays in the config folder.
- Fix: do the removal in the same task chain as the write.

## Low

| # | Location | Issue | Fix |
|---|---|---|---|
| L1 | `workspace-model/src/tabs_state.rs:254-256` | When a group fails to parse, `?` returns before its tabs go through `self.read`. `TabReader::seen` then lags behind, and the wrong tab, or none, is selected after restore. | Read the tabs before deserializing the group, or add `tabs.len()` to `seen`. |
| L2 | `app/src/hooks.rs:248` | `thread::spawn` panics if the OS refuses a thread, and this now runs on the UI thread every time a tab closes. | Use `thread::Builder::spawn`, as `agent-status/src/ipc.rs` now does. |
| L3 | `remote/src/transport.rs:63` | `connection_accepted()` runs before `from_same_user`. Another local user can keep connecting and stop the daemon from ever idling out. | Move the call after the check. |
| L4 | `remote/src/protocol.rs:612-615` | `cell_width` / `cell_height` aren't clamped; alacritty computes `ws_col * cell_width` in u16, which overflows at e.g. 1000 columns × 66 px. | Clamp them so that columns × cell size ≤ `u16::MAX`. |
| L5 | `remote/src/protocol.rs:601` | `count.min(len / 4)` can still reserve about 500 MB of `SessionInfo` items. | Cap the reservation, e.g. `.min(1024)`. |
| L6 | `remote/src/client.rs:156-174` | If the handshake finishes just as the watchdog kills ssh, a client whose connection is already dead is returned. | Check `timed_out` first. |
| L7 | `app/src/git_page/branch_picker.rs:351-357` | Picking the current branch on a dirty tree shows "Commit or discard…" although nothing would run. | Check `SwitchPlan::Nothing` before the blocker. |
| L8 | `app/src/git_page.rs` (`render_header`) | If the status turns into an error while the branch picker is open, `picker_open` stays true. Tab then stops moving focus, and the picker reopens by itself when the status comes back. | Close the picker in `take_status` when the status is `None`. |
| L9 | `app/src/git_page.rs:612-613` | `toggle_file`'s doc line now sits above `file_index`. | Move it back. |
| L10 | `app/src/file_viewer.rs:1234-1294` | The regular-file check (metadata) runs before the open, so a path swapped for a FIFO in between can still hang a background thread. | Check `metadata()` on the opened handle; `O_NONBLOCK` on Unix. |
| L11 | `app/src/workspace_view/git_pane/branch_switch.rs:79-85` | The unsaved-viewer check isn't filtered by `host`, so a remote tab's file is checked against the local repository. | Apply the same host filter. |
| L12 | `term-core/src/snapshot.rs` | The comment there says the scroll region (DECSTBM) is still not restored, but `docs/known-limitations.md` doesn't say so. | Add it there. |

## Previous review's fixes that check out

- Remote: the socket owner is checked in `connect`; pastes are split into 1 MiB pieces and a
  failed write disconnects; resize takes its locks in one order; the connection count is a drop
  guard; the stderr wait is bounded; the snapshot sets origin mode before moving the cursor.
- Git: Discard all runs `git clean` only on the counted untracked paths (`:(top,literal)`,
  chunked) and is refused while the status is truncated; write commands are never killed;
  trailing spaces are kept; change marks stop at the top folder; rows find their file by path.
- Hooks / state: random socket and file names, a sweep that continues past an unreadable entry,
  `check_private_dir`, the 2 s grace for an empty tabs lock, `resume_args` refusing ids that
  start with `-`, `windows_hook_command` refusing characters a shell would expand, zsh
  quoting of `"$dir"` and `"$file"`, scripts replaced through a temp file, `rename_into_place`.
- UI: the Markdown viewer's scheme allowlist, `quote_posix` backslashes, viewer reads capped and
  limited to regular files, history clear off the UI thread, Active→quiet after a wakeup with
  no output, chorded releases, backdrop retry, folder-watcher dedup.

## Good patterns worth keeping

- `ConnectionCount`: a guard that restores the connection count and idle time even after a
  panic.
- `rename_into_place`: one shared function that syncs, renames and cleans up the temp file on
  failure.
- `TabReader`: reads saved tabs one at a time, so one unknown entry doesn't lose them all.
- `arg_chunks`: keeps each git command under the Windows command-line limit, with a unit test.
- Branch switching always passes `--no-guess` and `--`, tested with a branch named like an
  option.
