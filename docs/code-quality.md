# Code quality

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
Small unit-test modules can stay inline.

Use integration tests for behavior exercised through a library's public API. A function
must be reachable through public modules or a `pub use` export; `pub fn` inside a private
module alone does not expose it outside the crate. Do not make private functions public
solely for testing.

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
`main`, `v*` tags and pull requests; a tag also publishes a GitHub release. They run no Clippy
yet.

To add it without ever blocking the build, Clippy gets its own job in `ubuntu.yml`. The `build`
job does not depend on it, so the binaries and the release are produced whether Clippy passes or
not. Its failure shows as a red check on the pull request; making it a required check in branch
protection turns it into a gate later, with no workflow change.

```yaml
  clippy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      # (same apt-get GPUI dependencies step as the build job)
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy }
      - uses: Swatinem/rust-cache@v2
      - run: cargo clippy --workspace --all-targets --locked -- -D warnings
```

Ubuntu alone is enough unless `#[cfg(windows)]` code needs linting too. Rejected: a Clippy step
with `continue-on-error: true` inside the `build` job, which keeps the job green so failures go
unnoticed in the log, and lengthens the build (Clippy cannot reuse the release build).

## Tool choice

The planned checks are GitHub-native, with no external service or secrets:

| Tool | Covers |
|---|---|
| Clippy | Lints and code smells |
| `cargo fmt --check` | Formatting |
| cargo-deny | Vulnerable dependencies (RustSec), licenses, duplicate crates |
| GitHub CodeQL | Security analysis, results in the repository's Security tab |
| cargo-llvm-cov (optionally with Codecov) | Test coverage |

SonarQube (Cloud is free for public repositories) supports Rust and adds a dashboard, history and
a quality gate, but its Rust analysis largely overlaps Clippy, whose reports it imports. It is
worth adding only if trends over time or a Sonar quality gate are wanted. DeepSource and Codacy
are hosted alternatives along the same lines.
