# Linux build and tests on a Windows machine

A native Windows build needs the MSVC linker (Visual Studio Build Tools, "Desktop development
with C++"). Without it `cargo` finds Git's `link.exe` and fails on the first build script
(`linking with link.exe failed`). The Linux build runs instead in Docker or in WSL Ubuntu,
with the same packages as `.github/workflows/ubuntu.yml`.

Either way the build is a Linux one: `#[cfg(windows)]` code is not compiled. The Windows CI
job (`windows.yml`) is what checks it; see [Windows code from Linux](#windows-code-from-linux).

## Docker

Used on 2026-10-20 to run `cargo test`, `cargo clippy` and `cargo fmt --check` for the
workspace (Docker Desktop, Linux engine).

`Dockerfile` (anywhere outside the repository):

```dockerfile
FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config libvulkan-dev libwayland-dev \
      libxkbcommon-x11-dev libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev \
      libssl-dev clang cmake git \
    && rm -rf /var/lib/apt/lists/*
RUN rustup component add clippy rustfmt
ENV CARGO_TARGET_DIR=/target
WORKDIR /src
```

`rust:1` is the latest stable, as CI's `dtolnay/rust-toolchain@stable`; the workspace needs
1.95 or later (`rust-version`). Build the image once, from the folder holding the `Dockerfile`:

```powershell
docker build -t bowarp-build .
```

Then, from PowerShell, run any cargo command against the working copy:

```powershell
docker run --rm `
    -v "C:\dev\Bowarp:/src" `
    -v bowarp-target:/target `
    -v bowarp-registry:/usr/local/cargo/registry `
    bowarp-build cargo test --workspace --locked
```

- The working copy is mounted at `/src`, so the tests see uncommitted changes.
- Build output goes to the `bowarp-target` volume and downloaded crates to `bowarp-registry`:
  later runs are incremental, and the Windows `target/` folder is left alone.
- The first run compiles every dependency (GPUI included) and takes several minutes.
- `cargo fmt --all` run this way rewrites files in the working copy (with LF line endings).
- From Git Bash, prefix the command with `MSYS_NO_PATHCONV=1`, or the `-v` paths are mangled.

Clean up with `docker rmi bowarp-build` and `docker volume rm bowarp-target bowarp-registry`.

## WSL Ubuntu

The same as on a Linux machine ([README](../README.md#build-and-run)). Targeted tests,
formatting and Clippy were run in WSL Ubuntu on 2026-10-24. Inside Ubuntu:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt install build-essential pkg-config libvulkan-dev libwayland-dev libxkbcommon-x11-dev \
  libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev libssl-dev clang cmake
cd /mnt/c/dev/Bowarp
CARGO_TARGET_DIR=~/bowarp-target cargo test --workspace --locked
```

`CARGO_TARGET_DIR` keeps the build on the Linux file system: building under `/mnt/c` is much
slower, and the Linux output would mix with the Windows `target/` folder. The sources can stay
on `C:`; cloning into the Linux home instead speeds up the build further.

Permission tests need temporary files on the Linux filesystem: a Windows-mounted directory
without DrvFS metadata cannot enforce Unix `0600` permissions. If `/tmp` is Windows-backed,
set `TMPDIR` to a private directory in the Linux home before running tests.

## Windows code from Linux

Not tried. `cargo check --target x86_64-pc-windows-msvc` type-checks the `#[cfg(windows)]` code
without linking, but crates that compile C (the tree-sitter grammars) need a C compiler for
the Windows target; [cargo-xwin](https://github.com/rust-cross/cargo-xwin) provides one (clang
with the MSVC headers it downloads) and can also build the `.exe`. The tests still need Windows
to run (ConPTY, the GPUI Windows backend), so a Windows run is either the `windows.yml` CI job
or a local build with the Visual Studio Build Tools installed.
