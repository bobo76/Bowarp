#!/usr/bin/env bash
# Run the app headless on an Xvfb display, drive it and take a screenshot. Linux and WSL.
#
#   scripts/xvfb-run.sh [options] [-- app args...]     (app args default to: shell)
#
#   --no-build      skip `cargo build --workspace`
#   --steps FILE    bash file sourced once the window shows, with DISPLAY, APP_PID and WID set
#                   (e.g. `xdotool type 'ls'; xdotool key Return; sleep 1`)
#   --shot FILE     write a PNG of the screen there at the end
#   --idle SECS     after the steps, print the app's CPU ticks per second over SECS (default 5)
#   --size WxH      screen size (default 1600x1000)
#
# Needs Xvfb, xdotool, python3 and a Vulkan driver (Mesa's software one works:
# sudo apt install xvfb xdotool mesa-vulkan-drivers). The app gets temporary XDG folders, so
# the real config is never touched. In a checkout under /mnt (WSL) the build goes to
# ~/.cache/bowarp-target unless CARGO_TARGET_DIR is set (/mnt is slow, target/ is Windows').
set -euo pipefail

build=1
steps=
shot=
idle=5
size=1600x1000
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) build=0 ;;
    --steps) steps=$(realpath "$2"); shift ;;
    --shot) shot=$(realpath -m "$2"); shift ;;
    --idle) idle=$2; shift ;;
    --size) size=$2; shift ;;
    --) shift; break ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done
[ $# -gt 0 ] || set -- shell

for tool in Xvfb xdotool python3; do
  command -v "$tool" > /dev/null || { echo "$tool not found (see the header)" >&2; exit 1; }
done

cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
case "$PWD" in
  /mnt/*) export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/.cache/bowarp-target} ;;
esac
target=${CARGO_TARGET_DIR:-target}
[ "$build" = 0 ] || cargo build --workspace

tmp=$(mktemp -d)
xvfb_pid=
APP_PID=
cleanup() {
  [ -z "$APP_PID" ] || kill "$APP_PID" 2> /dev/null || true
  [ -z "$xvfb_pid" ] || kill "$xvfb_pid" 2> /dev/null || true
  rm -rf "$tmp"
}
trap cleanup EXIT

# A free display by its lock file: `-displayfd` fails under WSLg, whose /tmp/.X11-unix is
# read-only (a fixed number still listens on the abstract socket).
display=99
while [ -e "/tmp/.X$display-lock" ]; do display=$((display + 1)); done
export DISPLAY=":$display"
unset WAYLAND_DISPLAY
Xvfb "$DISPLAY" -screen 0 "${size}x24" -fbdir "$tmp" > "$tmp/xvfb.log" 2>&1 &
xvfb_pid=$!
for _ in $(seq 50); do xdotool getdisplaygeometry > /dev/null 2>&1 && break; sleep 0.1; done
xdotool getdisplaygeometry > /dev/null 2>&1 \
  || { tail -n 5 "$tmp/xvfb.log" >&2; echo "Xvfb did not start on $DISPLAY" >&2; exit 1; }

XDG_CONFIG_HOME=$tmp/config XDG_DATA_HOME=$tmp/data XDG_STATE_HOME=$tmp/state \
  XDG_CACHE_HOME=$tmp/cache "$target/debug/agent-workspace" "$@" > "$tmp/app.log" 2>&1 &
APP_PID=$!

WID=
for _ in $(seq 100); do
  kill -0 "$APP_PID" 2> /dev/null || break
  WID=$(xdotool search --onlyvisible --pid "$APP_PID" 2> /dev/null | head -n 1 || true)
  [ -z "$WID" ] || break
  sleep 0.2
done
if [ -z "$WID" ]; then
  cat "$tmp/app.log" >&2
  echo "the app's window did not show" >&2
  exit 1
fi
echo "display $DISPLAY, app pid $APP_PID, window $WID"
sleep 2

if [ -n "$steps" ]; then
  export DISPLAY APP_PID WID
  # shellcheck source=/dev/null
  source "$steps"
fi

if [ "$idle" != 0 ]; then
  ticks() { awk '{print $14 + $15}' "/proc/$APP_PID/stat"; }
  before=$(ticks)
  sleep "$idle"
  per_sec=$(( ($(ticks) - before) / idle ))
  echo "idle CPU: $per_sec ticks/s ($(getconf CLK_TCK) ticks = 1 core-second)"
fi

if [ -n "$shot" ]; then
  python3 -I - "$tmp/Xvfb_screen0" "$shot" << 'EOF'
import struct, sys, zlib
data = open(sys.argv[1], "rb").read()
head = struct.unpack(">25I", data[:100])
header_size, width, height, bytes_per_line, ncolors = head[0], head[4], head[5], head[12], head[19]
start = header_size + ncolors * 12
rows = bytearray()
for y in range(height):
    line = data[start + y * bytes_per_line : start + y * bytes_per_line + width * 4]
    rows.append(0)
    for x in range(0, width * 4, 4):
        rows += bytes((line[x + 2], line[x + 1], line[x]))
def chunk(kind, body):
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
open(sys.argv[2], "wb").write(
    b"\x89PNG\r\n\x1a\n"
    + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(bytes(rows), 6))
    + chunk(b"IEND", b"")
)
EOF
  echo "screenshot: $shot"
fi

kill -0 "$APP_PID" 2> /dev/null || { cat "$tmp/app.log" >&2; echo "the app exited" >&2; exit 1; }
