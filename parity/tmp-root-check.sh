#!/bin/sh
# --tmp-root DIR must leave DIR and everything already in it alone, an earlier
# --keep-tmp run included: the harness works in, and removes, only the
# DIR/ccsl-parity-XXXXXX it creates. Runs one case, so it
# needs what the harness needs (the release build, upstream installed). Extra
# args go to run.mjs.
set -eu
here=$(dirname "$0")
here=$(CDPATH= cd -- "$here" && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
echo keep > "$root/unrelated.txt"
# An earlier --keep-tmp run whose pid the harness now reuses: exec keeps the
# shell's pid, so ccsl-parity-$$ is named after the harness's own pid.
sh -c 'mkdir "$1/ccsl-parity-$$" && echo keep > "$1/ccsl-parity-$$/kept.txt" && shift && exec node "$@"' \
    sh "$root" "$here/run.mjs" -q --tmp-root "$root" --filter core/model-string "$@"
if [ ! -f "$root/unrelated.txt" ]; then
    echo "FAIL: --tmp-root removed a file it did not create" >&2
    exit 1
fi
set -- "$root"/ccsl-parity-*/kept.txt
if [ ! -f "$1" ]; then
    echo "FAIL: --tmp-root removed an earlier run's scratch" >&2
    exit 1
fi
kept=$(basename "$(dirname "$1")")
leftover=$(ls -A "$root" | grep -v -e '^unrelated\.txt$' -e "^$kept\$" || true)
if [ -n "$leftover" ]; then
    echo "FAIL: --tmp-root left its scratch behind: $leftover" >&2
    exit 1
fi
echo "ok   --tmp-root kept unrelated files and removed only its own scratch"
