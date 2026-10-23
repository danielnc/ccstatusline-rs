#!/bin/sh
# Clean repo on branch feature/login, origin https://github.com/acme/widgets.git,
# no git-review cache (for --internal-refresh-git-review-cache cases).
. "$(dirname "$0")/_gitlib.sh"
g init -q "$1"
printf 'review\n' > "$1/README.md"
g -C "$1" add README.md
commit "$1" "Initial commit"
g -C "$1" checkout -q -b feature/login
g -C "$1" remote add origin https://github.com/acme/widgets.git
