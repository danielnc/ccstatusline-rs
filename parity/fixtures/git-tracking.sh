#!/bin/sh
# No `upstream` remote: main tracks my/remote/main, where remotes `my` and
# `my/remote` both exist (longest remote name wins). origin is acme/widgets,
# `fork` is someone/widgets, my/remote is other-org/thing.
. "$(dirname "$0")/_gitlib.sh"
g init -q "$1"
printf 'x\n' > "$1/x.txt"
g -C "$1" add x.txt
commit "$1" "Initial commit"
g -C "$1" remote add origin git@github.com:acme/widgets.git
g -C "$1" remote add fork https://github.com/someone/widgets
g -C "$1" remote add my https://gitlab.com/my/short.git
# Git 2.51+ refuses `remote add` for a name nested under an existing remote
# ("is a subset of existing remote"), so write the two keys `remote add`
# would have written. Older and newer git read the result the same way.
g -C "$1" config remote.my/remote.url ssh://git@github.com/other-org/thing.git
g -C "$1" config remote.my/remote.fetch "+refs/heads/*:refs/remotes/my/remote/*"
g -C "$1" update-ref refs/remotes/my/remote/main HEAD
g -C "$1" config branch.main.remote my/remote
g -C "$1" config branch.main.merge refs/heads/main
