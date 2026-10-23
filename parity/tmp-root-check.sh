#!/bin/sh
# --tmp-root DIR must leave DIR and everything already in it alone: the harness
# works in, and removes, only its own DIR/ccsl-parity-<pid>. Runs one case, so it
# needs what the harness needs (the release build, upstream installed). Extra
# args go to run.mjs.
set -eu
here=$(dirname "$0")
here=$(CDPATH= cd -- "$here" && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
echo keep > "$root/unrelated.txt"
node "$here/run.mjs" -q --tmp-root "$root" --filter core/model-string "$@"
if [ ! -f "$root/unrelated.txt" ]; then
    echo "FAIL: --tmp-root removed a file it did not create" >&2
    exit 1
fi
leftover=$(ls -A "$root" | grep -v '^unrelated\.txt$' || true)
if [ -n "$leftover" ]; then
    echo "FAIL: --tmp-root left its scratch behind: $leftover" >&2
    exit 1
fi
echo "ok   --tmp-root kept unrelated files and removed only its own scratch"
