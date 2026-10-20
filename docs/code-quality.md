# Code quality

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
