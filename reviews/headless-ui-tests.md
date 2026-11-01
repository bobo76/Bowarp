# Cheaper headless UI tests (proposal)

Not implemented. The `app` crate has headless GPUI tests (`#[gpui::test]`, 74 tests in 15 files,
26 in `workspace_view/tests.rs`), but each one repeats the same setup and drives the UI through
`WorkspaceView` internals rather than the elements a user clicks. A shared fixture and input
helpers make each new UI test cheaper to write.

Decided: do Part A only. The automation server in the running app (Part B, below) is deferred.

## Options considered

1. **GPUI headless tests (chosen).** Scripted UI tests with `TestAppContext` /
   `VisualTestContext` under `cargo test` (already in use: `gpui-pre` with `test-support` is a
   dev-dependency of `app`). Cheapest repeatable regression coverage, runs in CI.
2. **Built-in automation server behind a Cargo feature** (Part B). Lets Claude Code drive a
   running app through a local socket. Most work; deferred.
3. **OS-level computer use.** No app changes, but slow, flaky, reads pixels rather than state
   (the custom terminal element has no accessibility tree) and cannot run headless in CI.

## Plan

Each step is one commit that passes the check.

A1. **Shared fixture.** New `crates/app/src/test_support.rs` (`#[cfg(test)] mod test_support;`
in `main.rs`), pulling out what every `#[gpui::test]` repeats today:
- `init_app(cx)`: `gpui_kit::init`, `settings_file::init_in_memory`, `terminal_element::init`,
  `terminal_view::init`, `workspace_view::init`, `settings_page::init_keyboard`,
  `history::init_in_memory`.
- `open_workspace(cx, tabs) -> (Entity<WorkspaceView>, VisualTestContext)`: a window with a
  `Root`, a `TabsWriter::for_test` whose snapshots come back on a channel, no hooks.
- `shell_tab(script)`: a `/bin/sh -c` `LaunchSpec` (`cmd /c` on Windows), and the existing
  `wait_for_exit` (`workspace_view/tests.rs`) moved here.
- Move the existing tests onto it, without changing what they assert. 9 tests call
  `settings_file::init`, which loads the real `settings.json` through `config_file` (against
  the "never touch the real config folder" rule) and makes them depend on the user's settings;
  the fixture uses `init_in_memory`. That skips `install`'s theme sync, so a test that needs it
  installs default settings through `install` instead.

A2. **Input helpers** in `test_support`, located by element ids:
- Ids: call GPUI's `.debug_selector(|| ...)` directly. It exists in every build and is a no-op
  outside test builds, and the app already uses it (`sidebar-tab-{n}`, `sidebar-header`,
  `explorer-row-{n}`, `explorer-header`, `file-explorer`, `file-viewer`, `settings-row`,
  `settings-background`). Add ids only as tests need them, following those names (e.g.
  `titlebar-settings`, `new-tab-button`, `menu-item-{label}`).
- `click(cx, id)`, `right_click(cx, id)`, `drag(cx, from_id, to_id)` (mouse down / move / up
  at the centers of `VisualTestContext::debug_bounds(id)`), `keys(cx, "ctrl-tab")`
  (`simulate_keystrokes`), plus `tab_names(cx)` / `selected_tab(cx)` read from `WorkspaceView`.
- An unknown id panics with the ids rendered in that frame.
- `debug_bounds` needs `debug_assertions`: tests using these helpers carry the same
  `#[cfg_attr(not(debug_assertions), ignore = "...")]` as the existing bounds test.

A3. **First tests on the helpers.** Pick UI behavior that is today only checked through
`WorkspaceView` internals or by hand: menus, tab drags, pin, groups, rename. New UI features
use the helpers from then on.

Limits: the test platform paints no pixels and has no window manager, and `app` tests that spawn
a pty are flaky under Wine (see docs/windows-testing.md). These tests won't clear the current
"Fixed, not tested yet" items in BACKLOG.md: those need Windows, real agents, a compositor or a
look at the pixels.

## Deferred: automation server in the running app (Part B)

A local-socket server behind an `automation` Cargo feature (off by default, so not in release
builds), turned on by `AGENTWS_AUTOMATION=<socket name>`, with a small `agentws-ctl` client.
Commands: `get_state`, `read_terminal`, `dispatch_action` (`App::build_action`), `type_text`,
`send_keys` (`Keystroke::parse` + `Window::dispatch_keystroke`) and `wait_for` (checked on
workspace notify and session `Wakeup`, not by polling). It needs its own request/reply socket:
the hook IPC (`agent-status/src/ipc.rs`) is one-way, one line of at most 512 bytes.

Not worth it yet: the Xvfb harness in `pipeline/tools/` already drives the app on Linux, and most
pending manual checks are visual or need real Windows and agents, where state queries don't help.
Revisit if pipeline agents keep failing on pixel coordinates, or to let Claude run checks on a
Windows machine; a read-only first step (`get_state`, `read_terminal`) would cost about half.
