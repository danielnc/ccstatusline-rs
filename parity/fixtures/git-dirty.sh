#!/bin/sh
# Staged + unstaged + untracked changes, with insertions and deletions.
. "$(dirname "$0")/_lib.sh"
target_dir "$@"
g init -q "$1"
mkdir -p "$1/src"
printf 'line 1\nline 2\nline 3\nline 4\nline 5\n' > "$1/src/app.txt"
printf '# Dirty\n\nSome text.\nMore text.\n' > "$1/README.md"
printf 'to be deleted\n' > "$1/old.txt"
g -C "$1" add -A
commit "$1" "Initial commit"
# Staged: modify app.txt (+2 -1), add a new file, delete old.txt
printf 'line 1\nline 2 changed\nline 3\nline 4\nline 5\nline 6\n' > "$1/src/app.txt"
printf 'brand new\nfile\n' > "$1/staged-new.txt"
g -C "$1" add src/app.txt staged-new.txt
g -C "$1" rm -q old.txt
# Unstaged: modify README (+3 -1) and app.txt again on top of the staged change
printf '# Dirty\n\nSome text.\nAdded a\nAdded b\nAdded c\n' > "$1/README.md"
printf 'line 1\nline 2 changed\nline 3\nline 4\nline 5\nline 6\nline 7\n' > "$1/src/app.txt"
# Untracked
printf 'u1\n' > "$1/untracked1.txt"
printf 'u2\n' > "$1/untracked2.txt"
mkdir -p "$1/untracked-dir"
printf 'u3\n' > "$1/untracked-dir/u3.txt"
