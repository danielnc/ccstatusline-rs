#!/bin/sh
# Measure peak RSS and wall time per render: an optional existing status line
# command, pinned upstream ccstatusline on the full payload, and
# ccstatusline-rs cold/warm, plus 60 concurrent ccstatusline-rs renders.
#
# usage: [OLD_CMD="sh /path/to/statusline.sh"] tools/bench.sh [transcript.jsonl] [runs]
# OLD_CMD, if set, is the status line command currently in use, measured as-is.
# MACHINE, if set, labels the results (default: OS, architecture and CPU model,
# never the hostname). TIME_CMD (default /usr/bin/time) is BSD time (-l, macOS)
# or GNU time (-f); any failed render or measurement stops the run.
# Uses a scratch HOME and a copy of the config (CONFIG, default
# ~/.config/ccstatusline/settings.json), so nothing real is written.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TRANSCRIPT=${1:-$(ls -S "$HOME"/.claude/projects/*/*.jsonl | head -1)}
RUNS=${2:-5}
BIN=$ROOT/target/release/ccstatusline-rs
UPSTREAM=$ROOT/parity/upstream/node_modules/ccstatusline/dist/ccstatusline.js
LIVE_CONFIG=${CONFIG:-$HOME/.config/ccstatusline/settings.json}
WORK=${TMPDIR:-/tmp}/ccsl-bench.$$
mkdir -p "$WORK/home/.config/ccstatusline"
trap 'rm -rf "$WORK"' EXIT
cp "$LIVE_CONFIG" "$WORK/config.json"

die() {
    echo "bench.sh: $*" >&2
    exit 1
}

TIME_CMD=${TIME_CMD:-/usr/bin/time}
if "$TIME_CMD" -l true > /dev/null 2>&1; then
    TIME_KIND=bsd
elif "$TIME_CMD" -f '%M %e' true > /dev/null 2>&1; then
    TIME_KIND=gnu
else
    die "$TIME_CMD supports neither -l (BSD time) nor -f (GNU time); set TIME_CMD"
fi

# Paths go into the payload JSON-encoded, so quotes and backslashes in them stay valid JSON.
json_str() {
    node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$1"
}
TRANSCRIPT_JSON=$(json_str "$TRANSCRIPT")
ROOT_JSON=$(json_str "$ROOT")

NOW_S=$(date +%s)
cat > "$WORK/payload.json" <<EOF
{"session_id":"bench-0001","transcript_path":$TRANSCRIPT_JSON,"cwd":$ROOT_JSON,
 "workspace":{"current_dir":$ROOT_JSON,"project_dir":$ROOT_JSON},
 "model":{"id":"claude-opus-5-5","display_name":"Opus 5.5"},"version":"2.1.306",
 "cost":{"total_cost_usd":12.34,"total_duration_ms":3600000,"total_api_duration_ms":1200000,"total_lines_added":120,"total_lines_removed":30},
 "context_window":{"context_window_size":1000000,"total_input_tokens":523000,"total_output_tokens":81000,
   "current_usage":{"input_tokens":1200,"output_tokens":800,"cache_creation_input_tokens":3000,"cache_read_input_tokens":250000},
   "used_percentage":25.4,"remaining_percentage":74.6},
 "rate_limits":{"five_hour":{"used_percentage":12,"resets_at":$((NOW_S + 7200))},"seven_day":{"used_percentage":40,"resets_at":$((NOW_S + 300000))}}}
EOF

# timed <out> <command...>: runs the command on the payload and writes "max_rss_bytes wall_s" to <out>; dies if the
# command fails or the time command reports no numbers
timed() {
    out=$1; shift
    case $TIME_KIND in
        bsd)
            "$TIME_CMD" -l "$@" < "$WORK/payload.json" > /dev/null 2> "$out.raw" || die "render failed: $*"
            awk '/maximum resident set size/ {rss=$1} / real / {real=$1} END {print rss, real}' "$out.raw" > "$out"
            ;;
        gnu)
            "$TIME_CMD" -f '%M %e' -o "$out.raw" "$@" < "$WORK/payload.json" > /dev/null 2>&1 || die "render failed: $*"
            awk 'NF == 2 {printf "%d %s\n", $1 * 1024, $2}' "$out.raw" > "$out"
            ;;
    esac
    read -r rss real < "$out" || true
    case ${rss:-} in '' | *[!0-9]*) die "$TIME_CMD reported no peak RSS for: $*" ;; esac
    case ${real:-} in '' | *[!0-9.]*) die "$TIME_CMD reported no wall time for: $*" ;; esac
}

# measure <label> <command...>: prints "label  max_rss_MB  wall_s" (median of RUNS)
measure() {
    label=$1; shift
    : > "$WORK/m.txt"
    i=0
    while [ $i -lt "$RUNS" ]; do
        timed "$WORK/t.txt" "$@"
        cat "$WORK/t.txt" >> "$WORK/m.txt"
        i=$((i + 1))
    done
    sort -n -k2 "$WORK/m.txt" | awk -v l="$label" -v n="$RUNS" 'NR==int((n+1)/2) {printf "%-34s %8.1f MB %8.3f s\n", l, $1/1048576, $2}'
}

cpu=$(sysctl -n machdep.cpu.brand_string 2> /dev/null || sed -n 's/^model name[[:space:]]*: //p' /proc/cpuinfo 2> /dev/null | head -1)
echo "machine: ${MACHINE:-$(uname -sm)${cpu:+, $cpu}}"
echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "transcript: $(basename "$TRANSCRIPT") ($(du -h "$TRANSCRIPT" | cut -f1)), median of $RUNS runs"

export HOME="$WORK/home" CCSTATUSLINE_WIDTH=200
if [ -n "${OLD_CMD:-}" ]; then
    # shellcheck disable=SC2086
    measure "old: $OLD_CMD" $OLD_CMD
fi
measure "upstream 2.2.32 (full payload)" node "$UPSTREAM" --config "$WORK/config.json"
rm -rf "$WORK/home/.cache"
RUNS_SAVE=$RUNS; RUNS=1
measure "ccstatusline-rs cold (first render)" "$BIN" --config "$WORK/config.json"
RUNS=$RUNS_SAVE
measure "ccstatusline-rs warm" "$BIN" --config "$WORK/config.json"

# 60 concurrent warm renders
start=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
pids=
i=0
while [ $i -lt 60 ]; do
    timed "$WORK/c$i.txt" "$BIN" --config "$WORK/config.json" &
    pids="$pids $!"
    i=$((i + 1))
done
for pid in $pids; do
    wait "$pid" || die "a concurrent render failed"
done
end=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
cat "$WORK"/c*.txt | awk -v s="$start" -v e="$end" '{sum+=$1; if ($1>max) max=$1} END {printf "%-34s %8.1f MB max, %6.1f MB total, %6.3f s wall\n", "ccstatusline-rs 60 concurrent", max/1048576, sum/1048576, e-s}'
