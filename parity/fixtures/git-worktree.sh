#!/bin/sh
# A main repo at <dir>/main and a linked worktree at <dir>/linked on branch
# feature/wt. Linked worktrees record absolute paths, so this fixture must be
# built in place (the runner always does).
. "$(dirname "$0")/_lib.sh"
target_dir "$@"
g init -q "$1/main"
printf 'main\n' > "$1/main/main.txt"
g -C "$1/main" add main.txt
commit "$1/main" "Initial commit"
g -C "$1/main" worktree add -q -b feature/wt "$1/linked"
printf 'wt change\n' >> "$1/linked/main.txt"
