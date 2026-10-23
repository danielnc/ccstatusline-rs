#!/bin/sh
# Harness soundness check: run the parity suite with upstream standing in for
# the Rust binary. Anything short of 100% means the harness (or the corpus) is
# nondeterministic, not that Rust is wrong. Extra args go to run.mjs.
set -eu
here=$(dirname "$0")
here=$(CDPATH= cd -- "$here" && pwd)
exec node "$here/run.mjs" --rust-bin "$here/upstream-cli.sh" -q "$@"
