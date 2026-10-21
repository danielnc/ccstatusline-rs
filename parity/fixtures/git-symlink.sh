#!/bin/sh
# A repo at <dir>/real with a subdirectory sub/ holding a modified file, and a
# symlink <dir>/link -> real/sub (cwd through a symlink).
. "$(dirname "$0")/_gitlib.sh"
g init -q "$1/real"
mkdir -p "$1/real/sub"
printf 'a\nb\n' > "$1/real/sub/file.txt"
g -C "$1/real" add -A
commit "$1/real" "Initial commit"
printf 'a\nb\nc\n' > "$1/real/sub/file.txt"
ln -s real/sub "$1/link"
