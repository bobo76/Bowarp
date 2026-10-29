# Automation door for testing (proposal)

Not implemented. The goal is to let Claude Code (or another MCP client) drive a running app and
check what it shows, so features can be tested without a person at the keyboard: notably the
"Fixed, not tested yet" items in [BACKLOG.md](../BACKLOG.md) that wait for a manual run on
Windows. The `app` crate has headless GPUI tests (`#[gpui::test]` in about a dozen modules, e.g.
`workspace_view/tests.rs`), but nothing can drive a running app except the pixel-coordinate
Xvfb scripts in the git-ignored `pipeline/tools/`.

## What a test door needs

| Capability | Tools | Why |
|---|---|---|
| Inspect state | `get_state`, `read_terminal` | Structured data to assert on: tabs (name, folder, agent status, attention), selected tab, sidebar / explorer visibility, open dialog, visible text of a tab |
| Act | `dispatch_action`, `send_keys`, `type_text` | The app's own actions (new / close / duplicate tab, toggle sidebar, open settings), keystrokes to the focused element, input to a tab's terminal |
| See | `screenshot` | Layout checks that state cannot show |
| Wait | `wait_for` | Wait until a tab is Done or the terminal shows a text, instead of fixed sleeps |

## Options

### 1. Built-in automation server behind a Cargo feature (recommended)

An `automation` feature adds a server to the app exposing the tools above over a local socket
(named pipe on Windows, Unix socket on Linux), and a thin stdio MCP bridge connects Claude Code
to it (`claude mcp add agentws -- agentws-mcp`).

- Release builds do not contain it, so typing into tabs (which could answer an agent's
  permission prompts) is not a security issue and does not stretch the "no prompt
  orchestration" scope in [FEATURES.md](../FEATURES.md).
- Asserts on exact state; works the same on Windows and Linux CI.
- Most work: each tool is wired to GPUI entities on the main thread.

### 2. OS-level computer use

Screenshots and clicks on the desktop, with no changes to the app. Works today, but it is slow,
flaky, reads pixels rather than state (the custom terminal element has no accessibility tree)
and cannot run headless in CI.

### 3. GPUI headless tests

Scripted UI tests with `TestAppContext` / `VisualTestContext` under `cargo test` (already in
use: `gpui-pre` with `test-support` is a dev-dependency of `app`). Not MCP, but the cheapest
repeatable regression coverage. It complements option 1: option 1 for
exploratory testing by Claude, option 3 for regressions in CI.

## Plan

Part A (option 3) comes first: it is cheap, and Part B's command handlers are tested with its
helpers. Each step is one commit that passes the check.

### Part A: cheaper headless UI tests

A1. **Shared fixture.** New `crates/app/src/test_support.rs` (`#[cfg(test)] mod test_support;`
in `main.rs`), pulling out what every `#[gpui::test]` repeats today:
- `init_app(cx)`: `gpui_kit::init`, `settings_file::init`, `terminal_element::init`,
  `terminal_view::init`, `workspace_view::init`, `settings_page::init_keyboard`,
  `history::init_in_memory`.
- `open_workspace(cx, tabs) -> (Entity<WorkspaceView>, VisualTestContext)`: a window with a
  `Root`, a `TabsWriter::for_test` whose snapshots come back on a channel, no hooks.
- `shell_tab(script)`: a `/bin/sh -c` `LaunchSpec` (`cmd /c` on Windows), and the existing
  `wait_for_exit` moved here.
- Move the existing tests onto it, without changing what they assert.

A2. **Element ids.** GPUI's `.debug_selector(..)` records an element's bounds for
`VisualTestContext::debug_bounds(..)`. It exists only with GPUI's `test-support` feature, which
`app` enables only as a dev-dependency, so a plain `cargo build` has no such method. Add a
`TestId` extension trait (`test_id(|| "tab-row-0".into())`) that calls `debug_selector` under
`#[cfg(test)]` and does nothing otherwise. First ids:
- title bar: `titlebar-sidebar`, `titlebar-explorer`, `titlebar-info`, `titlebar-settings`
- sidebar: `tab-row-{n}`, `group-header-{n}`, `new-tab-button`
- menus: `menu-item-{label}` (`+` menu, tab menu, group menu)
- `explorer-divider`, `settings-search`, `settings-row-{key}`

A3. **Input helpers** in `test_support`: `click(cx, id)`, `right_click(cx, id)`,
`drag(cx, from_id, to_id)` (mouse down / move / up at the bounds' centers), `keys(cx, "ctrl-tab")`
(`simulate_keystrokes`), plus `tab_names(cx)` / `selected_tab(cx)` read from `WorkspaceView`.
An unknown id panics with the ids rendered in that frame.

A4. **First tests.** Pick the BACKLOG.md "Fixed, not tested yet" items that are pure UI logic
(menus, drags, pin, groups, rename) and need neither a window manager nor Windows. Their manual
check then only covers what headless tests can't: pixels, real fonts, the WM, ConPTY.

Limits: the test platform paints no pixels, has no window manager, and `app` tests that spawn a
pty are flaky under Wine (see docs/windows-testing.md).

### Part B: automation server in the running app

B1. **Protocol in `workspace-model`** (`automation.rs`, pure, unit-tested). One JSON request
line and one JSON reply line per connection, at most 64 KiB:
```
{"cmd":"get_state"}
{"cmd":"read_terminal","tab":0}
{"cmd":"dispatch_action","name":"workspace::NewTab"}
-> {"ok":true,"result":{...}}  |  {"ok":false,"error":"unknown action: workspace::Nope"}
```
`AppSnapshot`: tabs (name, folder, kind, agent status, attention, pinned, group), selected
index, sidebar / explorer visible, open page (settings, file viewer) and open menu or dialog.
Tests cover parsing, unknown commands, oversize lines and the snapshot's JSON form.

B2. **Server** (`crates/app/src/automation.rs`, behind an `automation` Cargo feature, off by
default). A listener thread on its own local socket (same `interprocess` setup and same-user
restriction as `agent-status/src/ipc.rs`) reads a request and sends it with a reply sender to
the main thread over a `futures` channel. A GPUI task on the workspace window answers it with
access to `WorkspaceView`. The handler is a plain function
`handle(request, &mut WorkspaceView, window, cx) -> Response`, tested headlessly with Part A's
fixture. The transport is tested with a stub handler.

B3. **Launch.** `AGENTWS_AUTOMATION=<socket name>` turns it on (no change to
`parse_launch_args`), so the harness picks the name and there's no pid to find. Without the
feature the variable is ignored. `scrub_inherited_agent_env` also removes it, so child
shells don't inherit it. The check adds
`cargo clippy --workspace --all-targets --features agent-workspace/automation -- -D warnings`,
and so does CI.

B4. **Read-only commands.**
- `get_state`: built from `WorkspaceView` fields.
- `read_terminal`: the visible grid of that tab's `Term` as lines, trailing blanks trimmed.
  Plus cursor position and `display_offset`.
- `screenshot` stays with `pipeline/tools/shot.sh` for now.

B5. **Client.** New crate `crates/agentws-ctl` (bin, deps: `interprocess`, `serde_json`):
`agentws-ctl <socket> get_state`, `agentws-ctl <socket> read_terminal 0`, printing the reply's
`result` and exiting non-zero on `ok: false`. Claude drives the app through Bash with it. A
stdio MCP bridge (`agentws-mcp`) is optional and comes later. `tests/round_trip.rs` runs it
against a stub server, like `agentws-hook`'s.

B6. **Input commands.**
- `dispatch_action`: `cx.build_action(name, None)`, dispatched from the focused element, as a
  key binding would be. An unknown name errors and lists the registered `workspace::` actions.
- `type_text {tab, text}`: `TerminalView::type_text` (raw bytes, as today).
- `send_keys {keys: "ctrl-tab"}`: `Keystroke::parse`, then `window.dispatch_keystroke`.

B7. **Waiting.** `wait_for {until: {tab_status | terminal_contains | tab_count}, timeout_ms}`
(at most 30 s). It is checked again on each workspace notify and session `Wakeup`, not by
polling, and answers with the state that matched or a timeout error.

B8. **Harness.** `pipeline/tools/launch.sh` sets `AGENTWS_AUTOMATION=agentws-test-<pid>` and
builds with `--features automation`. The Xvfb memory note points at `agentws-ctl` and no
longer at coordinates, except for pixel checks.

### Open choices

- Gate: Cargo feature (recommended: compiled out of release builds, extra clippy run) vs
  `cfg(debug_assertions)` (no extra build, but every debug build has the door).
- Turn-on: environment variable (recommended: no argument parser change, set by the harness)
  vs `--automation <name>` flag (visible in `--help`, needs `parse_launch_args` changes).
- Client: its own crate (recommended) vs a second `[[bin]]` in `agentws-hook` (fewer crates,
  but it mixes a release binary with a dev tool).

## Reuse from the hook IPC

`agent-status/src/ipc.rs` already has a per-process local socket restricted to the same user.
It is one-way (one line of at most 512 bytes per connection), so the automation server needs
request/response framing (e.g. one JSON request and one JSON reply per line) on its own socket
rather than extending the hook protocol.
