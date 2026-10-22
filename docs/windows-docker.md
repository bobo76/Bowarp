# Windows build & tests in Docker (`dockur/windows`)

Runs a real Windows VM inside a container (QEMU/KVM), so ConPTY and the GPUI
(Direct3D 11) window behave as on a real machine. Use it when the cross-compile
check (`cargo xwin`) is not enough.

## Prerequisites

- Docker, and KVM on the host: `ls /dev/kvm` (user in the `kvm` group)
- ~40 GB free disk, 8 GB RAM, 4 cores to spare
- Windows licence, or the evaluation edition that `dockur/windows` downloads (time-limited)

## `compose.yaml` (keep outside the repo or add to `.gitignore`)

```yaml
services:
  windows:
    image: dockurr/windows
    container_name: windows
    environment:
      VERSION: "11"          # or "11e" (Enterprise eval), "10"
      RAM_SIZE: "8G"
      CPU_CORES: "4"
      DISK_SIZE: "64G"
    devices:
      - /dev/kvm
      - /dev/net/tun
    cap_add:
      - NET_ADMIN
    ports:
      - 8006:8006            # web viewer: http://localhost:8006
      - 3389:3389/tcp        # RDP
      - 3389:3389/udp
    volumes:
      - ./windows-storage:/storage        # VM disk (large, do not commit)
      - /home/bobo/dev/agent-workspace:/shared   # appears as \\host.lan\Data in Windows
    stop_grace_period: 2m
```

Start: `docker compose up -d`, then watch the install at http://localhost:8006.

## One-time setup inside Windows

In PowerShell (admin):

```powershell
winget install --id Rustlang.Rustup -e
winget install --id Microsoft.VisualStudio.2022.BuildTools -e `
  --override "--passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id Git.Git -e
rustup default stable-x86_64-pc-windows-msvc
```

Rust MSRV is 1.88 (see project memory); stable satisfies it.

## Build & test

Do not build on the shared folder (slow, and it would mix `target/` with the
Linux one). Copy or clone to a local drive:

```powershell
robocopy \\host.lan\Data C:\src\agent-workspace /MIR /XD target .git
cd C:\src\agent-workspace
cargo test --workspace
```

Or `git clone` the repo and fetch the branch instead of copying.

## Notes

- Take a VM snapshot/copy of `windows-storage` after setup so the toolchain
  install is not repeated.
- Stop with `docker compose stop` (graceful Windows shutdown); `down` keeps the
  disk in `./windows-storage`.
- Quick check without a VM (cross-compile + Wine): see [windows-testing.md](windows-testing.md).
