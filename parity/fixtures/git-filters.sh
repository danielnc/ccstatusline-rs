#!/bin/sh
# Modified tracked files under a repository-defined filter driver that would
# write $HOME/FILTER-RAN if git ran it, an lfs filter (kept by upstream), and a
# config include. status/diff must not run the evil driver.
. "$(dirname "$0")/_gitlib.sh"
g init -q "$1"
printf 'one\n' > "$1/a.txt"
printf 'lfs\n' > "$1/b.bin"
printf '*.txt filter=evil\n*.bin filter=lfs\n' > "$1/.gitattributes"
g -C "$1" add -A
commit "$1" "Initial commit"
printf 'one\ntwo\n' > "$1/a.txt"
printf 'lfs changed\n' > "$1/b.bin"
printf '[filter "extra"]\n\tsmudge = sh -c "touch \\"$HOME/EXTRA-RAN\\"; cat"\n' > "$1/.git/extra.config"
cat >> "$1/.git/config" <<CFG
[filter "evil"]
	clean = sh -c 'touch "\$HOME/FILTER-RAN"; cat'
	smudge = sh -c 'touch "\$HOME/FILTER-RAN"; cat'
	required = true
[filter "lfs"]
	clean = git-lfs clean -- %f
	smudge = git-lfs smudge -- %f
	process = git-lfs filter-process
[include]
	path = extra.config
CFG
