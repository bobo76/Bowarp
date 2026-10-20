# Automation door for testing (proposal)

Not implemented. The goal is to let Claude Code (or another MCP client) drive a running app and
check what it shows, so features can be tested without a person at the keyboard: notably the
"Fixed, not tested yet" items in [PLAN.md](../PLAN.md) that wait for a manual run on Windows. The
app has no UI tests today.

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

Scripted UI tests with `TestAppContext` / `VisualTestContext` under `cargo test`, if `gpui-kit`
exposes GPUI's `test-support`. Not MCP, but the cheapest repeatable regression coverage. It
complements option 1: option 1 for exploratory testing by Claude, option 3 for regressions in CI.

## Plan for option 1

1. Read-only: `get_state`, `read_terminal`, `screenshot`.
2. Actions: `dispatch_action`, `send_keys`, `type_text`.
3. `wait_for`.
4. Launch: an `--automation` flag and a pipe name the bridge can find; today's name
   (`agentws-<pid>.sock`) contains the process id.

## Reuse from the hook IPC

`agent-status/src/ipc.rs` already has a per-process local socket restricted to the same user.
It is one-way (one line of at most 512 bytes per connection), so the automation server needs
request/response framing (e.g. one JSON request and one JSON reply per line) on its own socket
rather than extending the hook protocol.
