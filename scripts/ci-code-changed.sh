#!/usr/bin/env bash
# Prints `code=true` when the diff between two commits touches anything other than
# documentation, `code=false` when it only touches documentation. CI skips the build, test and
# lint jobs on `code=false` (a skipped required check counts as passed).
#
# usage: scripts/ci-code-changed.sh <base> <head>
# With no arguments, or when the diff can't be listed, it prints `code=true`.
set -euo pipefail

# Documentation only: nothing the build reads. Keep this list tight; a file wrongly listed
# here lets a code change merge untested.
is_doc() {
    case "$1" in
        *.md | docs/* | reviews/* | .claude/*) return 0 ;;
        *) return 1 ;;
    esac
}

if [ $# -ne 2 ]; then
    echo code=true
    exit 0
fi
if ! files=$(git diff --name-only "$1...$2"); then
    echo code=true
    exit 0
fi
if [ -z "$files" ]; then
    echo code=true
    exit 0
fi
while IFS= read -r file; do
    if ! is_doc "$file"; then
        echo code=true
        exit 0
    fi
done <<<"$files"
echo code=false
