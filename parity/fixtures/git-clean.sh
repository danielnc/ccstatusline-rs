#!/bin/sh
# One commit, clean work tree, no remotes.
. "$(dirname "$0")/_lib.sh"
target_dir "$@"
g init -q "$1"
printf '# Widgets\n\nA clean repo.\n' > "$1/README.md"
g -C "$1" add README.md
commit "$1" "Initial commit"
