#!/bin/sh
# git init with no commits (unborn main branch) and one untracked file.
. "$(dirname "$0")/_lib.sh"
target_dir "$@"
g init -q "$1"
printf 'untracked\n' > "$1/new.txt"
