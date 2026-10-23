#!/bin/sh
# Clean repo on main, origin https://github.com/acme/widgets.git. The git-review
# cache file for this cwd and branch is a symlink to <dir>.review.json, so a
# case supplies the cache content with `files` (missing = no cache).
. "$(dirname "$0")/_gitlib.sh"
g init -q "$1"
printf 'review\n' > "$1/README.md"
g -C "$1" add README.md
commit "$1" "Initial commit"
g -C "$1" remote add origin https://github.com/acme/widgets.git
hash=$(printf '%s\0%s' "$1" "branch:main" | shasum -a 256 | cut -c1-16)
mkdir -p "$HOME/.cache/ccstatusline/git-review"
ln -s "$1.review.json" "$HOME/.cache/ccstatusline/git-review/git-review-$hash.json"
