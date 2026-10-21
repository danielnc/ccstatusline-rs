#!/bin/sh
# Staged rename and copy-like add, a staged type change, files with spaces and
# non-ASCII names, unstaged deletes and untracked files, so status -z parsing
# sees two-path entries.
. "$(dirname "$0")/_gitlib.sh"
g init -q "$1"
printf 'content one\nline\n' > "$1/old name.txt"
printf 'keep\n' > "$1/keep.txt"
printf 'gone\n' > "$1/gone.txt"
printf 'link target\n' > "$1/target.txt"
printf 'naïve\n' > "$1/naïve file.txt"
g -C "$1" add -A
commit "$1" "Initial commit"
g -C "$1" mv "old name.txt" "new name.txt"
cp "$1/keep.txt" "$1/kept copy.txt"
g -C "$1" add "kept copy.txt"
rm "$1/target.txt"
ln -s keep.txt "$1/target.txt"
g -C "$1" add target.txt
rm "$1/gone.txt"
printf 'naïve\nchanged\n' > "$1/naïve file.txt"
printf 'u\n' > "$1/ünïcode untracked.txt"
printf 'u\n' > "$1/untracked.txt"
