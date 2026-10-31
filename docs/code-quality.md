# Code quality

## File and module layout

- Workspace: every crate sits one level deep under `crates\`, in a folder named exactly like the
  crate.
- Module files: use `foo.rs` as the module root with its children in `foo\`. Do not add new
  `mod.rs` files; existing ones stay unless the module is reworked anyway.
- Order inside a file: `mod` declarations, then `use`, then public items before private ones,
  types before their `impl`s and functions. Write for a reader opening the file for the first
  time.
- Split by concept, not by line count: a new module should own a real concept (a type and its
  logic, a pane, a parser), not just hold the second half of a long file. Avoid many tiny
  modules that each need opening to follow one flow.
- Pure, non-UI logic goes in `workspace-model`, where it can be unit-tested without GPUI.

## Size limits

Rust has no official file or function length rule; these are this repository's limits.

| Measure | Limit | Enforced by |
|---|---|---|
| Function body | 100 lines | `clippy::too_many_lines`, threshold in `clippy.toml` |
| Function arguments | 7 | `clippy::too_many_arguments`, threshold in `clippy.toml` |
| File, production code only | About 800 lines (soft) | Review |
| Nesting | Early returns; nested helper functions at most one level deep | Review |

`too_many_lines` is a pedantic lint, enabled for every crate through
`[workspace.lints.clippy]` in the root `Cargo.toml`; each crate opts in with
`[lints] workspace = true`, which a new crate must copy. Test code is exempt: each crate root
has `#![cfg_attr(test, allow(clippy::too_many_lines))]`, because a long test scenario reads
better as one function. The normal (non-test) build is still checked, so production code stays
covered.

A function over the limit is split, not silenced. When splitting is not worth it yet, mark it
with `#[expect(clippy::too_many_lines, reason = "...")]`: `expect` warns once the function
shrinks below the limit, so the marker cannot outlive its cause. The two pre-existing cases,
`WorkspaceView::render` and `TerminalElement::prepaint`, are marked this way to be split the
next time they change.

Clippy has no file length lint. Past about 800 lines of production code, consider splitting
along a concept boundary; tests in a separate `tests.rs` do not count. Several app files are
already above it (`workspace_view.rs` most of all): shrink them when touching them rather than
in one sweep.

Other code shape rules:

- Prefer early returns to nested `if`/`match` blocks.
- Replace several `bool` or `Option` parameters with a config struct. If a `bool` argument is
  always the same literal at call sites, split the function in two.
- A helper used once can be a block in its caller rather than a separate function.

## Rust test organization

Inline unit tests are idiomatic Rust, but large test suites can live in separate files:

| Test type | Location | Access |
|---|---|---|
| Unit tests | A child module, such as `src\parser\tests.rs` for `src\parser.rs` | Parent module's private items via `use super::*;` |
| Integration tests | The crate's `tests\` directory, alongside its `src\` directory | Library's public API only |

For separate unit tests, keep this declaration in the production module:

```rust
#[cfg(test)]
mod tests;
```

Put `use super::*;` and the `#[test]` functions in the child module's `tests.rs`.
For a module defined in `mod.rs`, put `tests.rs` beside that file.
Prefer this arrangement when inline tests make production code harder to navigate.
Large suites now follow this layout, for example in
`crates\workspace-model\src\settings\tests.rs` and `crates\term-core\src\prompt\tests.rs`.
Small unit-test modules (under about 100 lines) can stay inline. Keep one `tests` module per
production module: platform-specific tests go inside it, gated with `#[cfg(windows)]` or a
`#[cfg(unix)] mod unix;` submodule, as in `crates\app\src\launcher\tests.rs`.

Use integration tests for behavior exercised through a library's public API. A function
must be reachable through public modules or a `pub use` export; `pub fn` inside a private
module alone does not expose it outside the crate. Do not make private functions public
solely for testing.

## Agent integrations

Agent definitions live in `crates\workspace-model\src\agents\`, one file per provider, and
are registered in `agents.rs`. Keep provider identity, launch metadata, capabilities, hook
artifacts, and protocol adapters in that definition. Shared launch, status, settings, and UI
code should consume the registry or capability/policy APIs rather than branching on provider
names. Add behavior coverage through a test-only provider when changing shared capability
consumers; tests must not make that provider available in production.

This repository already has integration tests in `crates\term-core\tests\session.rs` and
`crates\agentws-hook\tests\round_trip.rs`. The latter tests a binary by running it rather
than importing its private functions.

Examples of public APIs with production callers across crate boundaries:

| API | Consumer |
|---|---|
| `workspace_model::next_attention` | The app's `jump_to_attention` in `workspace_view.rs` |
| `workspace_model::file_tree::sort_entries` | The app's `read_folder` in `file_explorer.rs` |
| `workspace_model::file_view::decode_text` | The app's `read_text` in `file_viewer.rs` |
| `agent_status::forward_hook` | The hook binary's `run` in `main.rs` |

Keep these APIs public for their consumers, independently of where their tests live.
Moving tests should not change production visibility or behavior.

## Clippy locally

Clippy is Rust's linter. Install it once with `rustup component add clippy`, then from the
repository root:

```
cargo clippy --workspace --all-targets
```

It only reports warnings; it changes nothing and fails nothing. `--all-targets` checks tests and
benches too. Variants:

| Command | Effect |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | Fail on any warning, as CI would |
| `cargo clippy --workspace --all-targets --fix --allow-dirty` | Apply the fixes Clippy marks as safe (review the diff) |

## CI

`.github/workflows/ubuntu.yml` and `windows.yml` build, test and package the app on pushes to
`main`, `v*` tags and pull requests; a tag also publishes a GitHub release.

`ubuntu.yml` also runs three quality jobs:

| Job | Command |
|---|---|
| `clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| `fmt` | `cargo fmt --all --check` |
| `deny` | `cargo deny check advisories bans licenses sources`, configured by `deny.toml` |

The `build` and `release` jobs do not depend on them, so the binaries and the release are
produced whether they pass or not. A failure shows as a red check on the pull request; making a
job a required check in branch protection turns it into a gate, with no workflow change.

Ubuntu alone is enough unless `#[cfg(windows)]` code needs linting too. Rejected: a Clippy step
with `continue-on-error: true` inside the `build` job, which keeps the job green so failures go
unnoticed in the log, and lengthens the build (Clippy cannot reuse the release build).

Run the same checks locally before pushing:

```
cargo fmt --all --check
cargo deny check
```

Install cargo-deny once with `cargo install --locked cargo-deny`. A new dependency under a
license missing from `deny.toml`'s `allow` list fails the `licenses` check: add the license if
it is acceptable. A RustSec advisory fails the `advisories` check even without a code change,
so a red `deny` job on an untouched branch usually means a dependency needs updating.

## Tool choice

The checks are GitHub-native, with no external service or secrets:

| Tool | Covers | Status |
|---|---|---|
| Clippy | Lints and code smells | In CI |
| `cargo fmt --check` | Formatting | In CI |
| cargo-deny | Vulnerable dependencies (RustSec), licenses, duplicate crates | In CI |
| GitHub CodeQL | Security analysis, results in the repository's Security tab | Planned |
| cargo-llvm-cov (optionally with Codecov) | Test coverage | Planned |

SonarQube (Cloud is free for public repositories) supports Rust and adds a dashboard, history and
a quality gate, but its Rust analysis largely overlaps Clippy, whose reports it imports. It is
worth adding only if trends over time or a Sonar quality gate are wanted. DeepSource and Codacy
are hosted alternatives along the same lines.
