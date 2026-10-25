# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Agent Workspace (repo "Bowarp") is a GPUI desktop terminal app for running Claude Code, Copilot CLI
and shells side by side in tabs, with per-tab agent status. User-facing behavior and shortcuts are
documented in README.md; the target spec is FEATURES.md; the backlog, next steps and pending
manual checks are in PLAN.md; known limitations of built features are in
docs/known-limitations.md.

## Commands

`cargo` may not be on PATH: `export PATH="$HOME/.cargo/bin:$PATH"`.

```
cargo build                                    # build the whole workspace (needed: agentws-hook must sit next to the app binary)
cargo run -p agent-workspace -- [shell|claude|copilot] [project-dir]
cargo test                                     # all tests
cargo test -p workspace-model settings::       # one crate, tests matching a path filter
cargo test -p term-core --test session         # one integration test file
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings   # CI runs this with --locked
cargo deny check                               # licenses/advisories, configured by deny.toml
node --test crates/app/copilot-extension/bridge.test.mjs  # Copilot extension tests
scripts/test-windows.sh [--build-only]         # cross-compile tests for Windows and run under Wine
```

"The check" before committing: `cargo fmt --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings`.
rustfmt does not wrap comments; keep lines ≤ 100 chars by hand.

Version bumps: `scripts/bump-version.sh [patch|minor|major]` (refuses a dirty `Cargo.toml`). Each
pushed feature is a patch bump tagged `v<version>`; a finished milestone bumps minor. The release
checklist is RELEASE.md (also update PLAN.md, CHANGELOG.md "Unreleased", README.md).

## Constraints

- Stack: Rust + GPUI + `alacritty_terminal`. Priorities: performance, then memory, then
  Windows, then Ubuntu; event-driven redraw (no work when idle). Only `app` depends on GPUI.
- Zed's `terminal` / `terminal_view` crates are GPL-3: reference only, never copy code from
  them (`gpui` and `alacritty_terminal` are Apache-2.0).
- Edition 2024, MSRV 1.95 (`rust-version` in root `Cargo.toml`): don't use newer std APIs.
- `alacritty_terminal` is pinned `=0.26.0`: `term-core/src/prompt.rs` mirrors its crate-private
  pty poll token; re-check on any bump (`session::tests` fails if wrong).
- Clippy runs with `-D warnings`: an API added before its caller needs
  `#[expect(dead_code, reason = "...")]`, removed by the step that adds the caller.
- Windows release builds have no stderr (`windows_subsystem`), so errors users must see need UI.
- Several app instances can run at once (only tabs are locked, `instance_lock.rs`) and users edit
  config files by hand: config writes must read-modify-write.
- Tests must never touch the real config folder (e.g. never call `shell_integration::attach` or
  `config_file`); use a temp dir.

## Architecture

Cargo workspace, `crates/*`:

- **`term-core`** — no UI. A pty-backed child process feeding an alacritty `Term`. The renderer
  locks the term to read the grid and is notified by a coalesced `SessionEvent::Wakeup` (acked via
  `Session::ack_wakeup`). `prompt.rs` wraps the pty (`MarkedPty`) to parse OSC 133 shell marks;
  `search` implements terminal search; `launch.rs` builds launch specs.
- **`workspace-model`** — no UI. All pure logic with unit tests: tab layout (pins, groups, order),
  attention/next-tab, sidebar search, git branch/diff, OSC 133 and OSC 9;4 parsing, command
  history, links in output, file tree/viewer decoding, pane widths, window/tabs persistence state,
  settings file (`settings.rs`: every field is `lenient`-deserialized to its type's Default, so
  non-zero defaults need a newtype). **New non-UI logic belongs here, not in `app`.**
- **`agent-status`** — turns agent events into `AgentStatus`. Claude Code is launched with
  generated `--settings` whose hooks run `agentws-hook`; Copilot gets a per-tab `--plugin-dir`
  plugin (PermissionRequest only) plus OSC 9;4 progress for Working/Idle. Events arrive over a
  local socket/pipe at the app's `HookServer`. Env vars `AGENTWS_PIPE`, `AGENTWS_SESSION`,
  `AGENTWS_HOOK` route them to the right tab.
- **`agentws-hook`** — tiny binary that forwards hook JSON to the owning session; always exits 0
  silently for Claude hooks. Tested by running the binary (`tests/round_trip.rs`).
- **`app`** (package `agent-workspace`) — the GPUI UI (`gpui-kit`). `workspace_view.rs` is the
  root view (sidebar, tab cards, settings pane, file explorer/viewer, dividers); it embeds the
  active `TerminalView` uncached, so any notify on it re-renders the terminal. `terminal_view.rs`
  + `terminal_element.rs` render and drive one session; `launcher.rs` discovers shells/agents and
  gives each a stable id (`bash`, `zsh`, `fish`, `pwsh`, `powershell`, `cmd`, `git-bash`,
  `wsl:<distro>`, `claude`, `copilot`) used by settings. `shell_integration.rs` writes the scripts
  in `crates/app/shell-integration/` to the config folder; they load the user's rc files and emit
  OSC 133 marks carrying a per-session `AGENTWS_MARK_TOKEN`. `settings_file.rs` is the app-side
  settings global (`settings(cx)`, `update(cx, f)` — once per gesture); `settings_page.rs` builds
  rows with `switch_row`/`bound_row`/`dropdown_row`/`slider_row`. `tabs_file.rs` saves/restores
  `tabs.json` on a background writer. `copilot-extension/` is an optional Copilot CLI extension
  (JS) reporting interaction mode.

## Conventions

- Tests: small suites inline; larger ones in a child `tests.rs` (`#[cfg(test)] mod tests;`, with
  `use super::*;`), one `tests` module per production module, platform tests gated inside it
  (e.g. `launcher/tests/unix.rs`). Integration tests in a crate's `tests/` use only public API;
  don't make private items public just for tests. See docs/code-quality.md.
- Windows-specific path logic: write pure functions taking a `windows: bool` so it is unit-tested
  on Linux (no Windows clippy cross-check available locally).
- Bytes typed into the PTY (`TerminalView::type_text`) are raw; paths go through
  `file_tree::quote_path` / `cd_command`.
- PLAN.md: remove items once fully done and verified; implemented-but-unchecked items go under
  "Fixed, not tested yet". It holds work to do only: no status, test counts or verification
  records; a new limitation of a built feature goes to docs/known-limitations.md. It may
  contain the user's uncommitted edits — stage only your hunks.
- CHANGELOG.md lists user-visible changes only, under "Unreleased" until release.
- `pipeline/` (git-ignored) holds the multi-agent pipeline's logs, retros and the Xvfb UI test
  harness (`pipeline/tools/`); the pipeline itself is described in PIPELINE-PLAN.md.
