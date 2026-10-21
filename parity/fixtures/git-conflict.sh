#!/bin/sh
# An unmerged merge: both branches changed the same line of conflict.txt, and a
# second file conflicts too.
. "$(dirname "$0")/_lib.sh"
target_dir "$@"
g init -q "$1"
printf 'shared line\n' > "$1/conflict.txt"
printf 'other base\n' > "$1/other.txt"
g -C "$1" add -A
commit "$1" "Base"
g -C "$1" checkout -q -b feature
printf 'feature version\n' > "$1/conflict.txt"
printf 'other feature\n' > "$1/other.txt"
g -C "$1" add -A
commit "$1" "Feature edit"
g -C "$1" checkout -q main
printf 'main version\n' > "$1/conflict.txt"
printf 'other main\n' > "$1/other.txt"
g -C "$1" add -A
commit "$1" "Main edit"
PARITY_COMMIT_N=$((PARITY_COMMIT_N + 1))
d=$(printf '2026-01-01T%02d:00:00Z' "$PARITY_COMMIT_N")
GIT_AUTHOR_DATE="$d" GIT_COMMITTER_DATE="$d" g -C "$1" merge -q feature >/dev/null 2>&1 || true
