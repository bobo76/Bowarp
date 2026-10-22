#!/usr/bin/env bash
# Cross-compile the workspace tests for Windows and run them under Wine.
#
#   scripts/test-windows.sh [--build-only] [cargo test args...]
#
# Needs cargo-xwin, clang, llvm-lib, wine and jq (see docs/windows-docker.md). Wine has no
# full ConPTY or Direct3D, so this proves it compiles and the logic tests pass, not PTY/UI behavior.
set -euo pipefail

target=x86_64-pc-windows-msvc
run=1
if [ "${1:-}" = "--build-only" ]; then
  run=0
  shift
fi

cd "$(dirname "$0")/.."

# cc-rs and embed-resource look for unversioned llvm-lib / llvm-rc; Ubuntu only ships llvm-<tool>-<N>.
shim=$(mktemp -d)
trap 'rm -rf "$shim"' EXIT
for tool in llvm-lib llvm-rc; do
  command -v "$tool" > /dev/null && continue
  found=$(ls /usr/lib/llvm-*/bin/"$tool" 2> /dev/null | sort -V | tail -n 1 || true)
  [ -n "$found" ] || { echo "$tool not found; install the llvm package" >&2; exit 1; }
  ln -s "$found" "$shim/$tool"
done
export PATH="$shim:$PATH"

for tool in cargo-xwin jq; do
  command -v "$tool" > /dev/null || { echo "$tool not found" >&2; exit 1; }
done
if [ "$run" = 1 ]; then
  command -v wine > /dev/null || { echo "wine not found (sudo apt install wine64)" >&2; exit 1; }
fi

exes=$(cargo xwin test --workspace --target "$target" --no-run --message-format=json "$@" \
  | jq -r 'select(.reason == "compiler-artifact" and .profile.test == true and .executable != null) | .executable')

if [ "$run" = 0 ]; then
  echo "BUILD ok: $(echo "$exes" | wc -l) test executables"
  exit 0
fi

export WINEDEBUG=-all
export WINEPREFIX="${WINEPREFIX:-$HOME/.cache/agent-workspace-wine}"

failed=0
for exe in $exes; do
  echo "== $(basename "$exe")"
  wine "$exe" || failed=1
done
exit "$failed"
