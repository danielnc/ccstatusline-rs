#!/bin/sh
# An unmerged merge with three conflicted files, one named with runs of spaces
# (ls-files --unmerged parsing joins split fields with single spaces).
. "$(dirname "$0")/_gitlib.sh"
g init -q "$1"
for f in "a  b.txt" "a b.txt" "c.txt"; do printf 'base\n' > "$1/$f"; done
g -C "$1" add -A
commit "$1" "Base"
g -C "$1" checkout -q -b other
for f in "a  b.txt" "a b.txt" "c.txt"; do printf 'other\n' > "$1/$f"; done
g -C "$1" add -A
commit "$1" "Other"
g -C "$1" checkout -q main
for f in "a  b.txt" "a b.txt" "c.txt"; do printf 'main\n' > "$1/$f"; done
g -C "$1" add -A
commit "$1" "Main"
g -C "$1" merge -q other >/dev/null 2>&1 || true
