#!/bin/sh
# Runs pinned upstream ccstatusline exactly as run.mjs does (node + preload.mjs).
# Used as a stand-in --rust-bin by self-check.sh, or by hand for exploring.
here=$(dirname "$0")
here=$(CDPATH= cd -- "$here" && pwd)
exec node --import "file://$here/preload.mjs" "$here/upstream/node_modules/ccstatusline/dist/ccstatusline.js" "$@"
