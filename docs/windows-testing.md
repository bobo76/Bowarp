# Running the Windows build and tests from Linux

```
scripts/test-windows.sh                # cross-compile all tests for Windows, run them under Wine
scripts/test-windows.sh --build-only   # compile only
scripts/test-windows.sh -p term-core   # extra args go to `cargo xwin test`
```

The script cross-compiles with `cargo-xwin` for `x86_64-pc-windows-msvc`, then runs every
test `.exe` with `wine` and exits non-zero if any fails. Wine data lives in
`~/.cache/agent-workspace-wine` (override with `WINEPREFIX`).

## One-time setup (Ubuntu)

```
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin
sudo apt install clang llvm lld wine64 jq
```

The first run downloads the Windows SDK and CRT into `~/.cache/cargo-xwin` (a few hundred MB).
The script adds `llvm-lib` and `llvm-rc` shims itself, because Ubuntu only ships the
versioned `llvm-lib-18` / `llvm-rc-18` names that `cc-rs` and `embed-resource` don't find.

## What it proves

- The whole workspace compiles for Windows, so `cfg(windows)` breakage shows up.
- Logic tests pass on the Windows target (paths, settings, history, term-core, ...).

## What it does not prove

Wine has incomplete ConPTY and no usable Direct3D 11 for GPUI, so PTY/OSC 133 behavior and
the window itself are not verified. For those use a real Windows VM:
[windows-docker.md](windows-docker.md), or a `windows-latest` CI job.

## Troubleshooting

- `NotAttempted("llvm-rc")` or `failed to find tool "llvm-lib"`: install the `llvm` package.
- Wine prints a first-run prefix setup message: harmless, it only happens once.
- Slow first run: it compiles the whole dependency tree for the second target into
  `target/x86_64-pc-windows-msvc/`.
