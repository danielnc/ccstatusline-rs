# Sourced by the git-* fixtures of the git widget group (self-contained so they
# work from a worktree's fixtures/ dir too). Same conventions as _lib.sh:
# no user/system config, fixed identities, commit N dated 2026-01-01 +N hours.
set -eu
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME="Parity Bot"
export GIT_AUTHOR_EMAIL="parity@example.com"
export GIT_COMMITTER_NAME="Parity Bot"
export GIT_COMMITTER_EMAIL="parity@example.com"
export TZ=UTC
export LC_ALL=C
unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR GIT_INDEX_FILE 2>/dev/null || true
GL_N=0
g() {
    git -c init.defaultBranch=main -c commit.gpgsign=false -c tag.gpgsign=false \
        -c core.autocrlf=false -c core.fsmonitor=false -c advice.detachedHead=false "$@"
}
commit() {
    GL_N=$((GL_N + 1))
    d=$(printf '2026-01-01T%02d:00:00Z' "$GL_N")
    GIT_AUTHOR_DATE="$d" GIT_COMMITTER_DATE="$d" g -C "$1" commit -q -m "$2"
}
if [ $# -lt 1 ] || [ -z "$1" ]; then
    echo "usage: $0 <target-dir>" >&2
    exit 2
fi
mkdir -p "$1"
