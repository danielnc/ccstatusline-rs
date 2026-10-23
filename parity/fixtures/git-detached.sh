#!/bin/sh
# Detached HEAD at the first of two commits.
. "$(dirname "$0")/_lib.sh"
target_dir "$@"
g init -q "$1"
printf 'one\n' > "$1/file.txt"
g -C "$1" add file.txt
commit "$1" "First"
printf 'two\n' > "$1/file.txt"
g -C "$1" add file.txt
commit "$1" "Second"
g -C "$1" checkout -q --detach HEAD~1
