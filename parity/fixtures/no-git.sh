#!/bin/sh
# A plain directory that is not inside any git repository.
. "$(dirname "$0")/_lib.sh"
target_dir "$@"
printf 'just a file\n' > "$1/notes.txt"
mkdir -p "$1/sub/dir"
