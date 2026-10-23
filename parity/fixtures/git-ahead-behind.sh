#!/bin/sh
# Branch main tracking origin/main, ahead 2 and behind 1. The bare remote used
# to set this up is deleted afterwards; origin then points at a GitHub URL and
# an `upstream` remote names a different owner (so it reads as a fork).
. "$(dirname "$0")/_lib.sh"
target_dir "$@"
scratch="$1/.parity-scratch"
mkdir -p "$scratch"
g init -q --bare "$scratch/remote.git"
g init -q "$1/repo"
printf 'base\n' > "$1/repo/base.txt"
g -C "$1/repo" add base.txt
commit "$1/repo" "Base"
g -C "$1/repo" remote add origin "$scratch/remote.git"
g -C "$1/repo" push -q -u origin main
g clone -q "$scratch/remote.git" "$scratch/other"
printf 'theirs\n' > "$scratch/other/theirs.txt"
g -C "$scratch/other" add theirs.txt
commit "$scratch/other" "Their change"
g -C "$scratch/other" push -q origin main
printf 'ours 1\n' > "$1/repo/ours1.txt"
g -C "$1/repo" add ours1.txt
commit "$1/repo" "Our change 1"
printf 'ours 2\n' > "$1/repo/ours2.txt"
g -C "$1/repo" add ours2.txt
commit "$1/repo" "Our change 2"
g -C "$1/repo" fetch -q origin
g -C "$1/repo" remote set-url origin https://github.com/acme/widgets.git
g -C "$1/repo" remote add upstream https://github.com/upstream-org/widgets.git
rm -rf "$scratch"
# Move the repo up so the fixture dir itself is the work tree
mv "$1/repo/.git" "$1/.git"
for f in "$1/repo/"*; do mv "$f" "$1/"; done
rmdir "$1/repo"
