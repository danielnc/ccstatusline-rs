#!/bin/sh
# Measure peak RSS and wall time per render: an optional existing status line
# command, pinned upstream ccstatusline on the full payload, and
# ccstatusline-rs cold/warm, plus 60 concurrent ccstatusline-rs renders.
#
# usage: [OLD_CMD="sh /path/to/statusline.sh"] tools/bench.sh [transcript.jsonl] [runs]
# OLD_CMD, if set, is the status line command currently in use, measured as-is.
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

NOW_S=$(date +%s)
cat > "$WORK/payload.json" <<EOF
{"session_id":"bench-0001","transcript_path":"$TRANSCRIPT","cwd":"$ROOT",
 "workspace":{"current_dir":"$ROOT","project_dir":"$ROOT"},
 "model":{"id":"claude-opus-5-5","display_name":"Opus 5.5"},"version":"2.1.306",
 "cost":{"total_cost_usd":12.34,"total_duration_ms":3600000,"total_api_duration_ms":1200000,"total_lines_added":120,"total_lines_removed":30},
 "context_window":{"context_window_size":1000000,"total_input_tokens":523000,"total_output_tokens":81000,
   "current_usage":{"input_tokens":1200,"output_tokens":800,"cache_creation_input_tokens":3000,"cache_read_input_tokens":250000},
   "used_percentage":25.4,"remaining_percentage":74.6},
 "rate_limits":{"five_hour":{"used_percentage":12,"resets_at":$((NOW_S + 7200))},"seven_day":{"used_percentage":40,"resets_at":$((NOW_S + 300000))}}}
EOF

# measure <label> <command...>: prints "label  max_rss_MB  wall_s" (median of RUNS)
measure() {
    label=$1; shift
    : > "$WORK/m.txt"
    i=0
    while [ $i -lt "$RUNS" ]; do
        /usr/bin/time -l "$@" < "$WORK/payload.json" > /dev/null 2> "$WORK/t.txt" || true
        rss=$(awk '/maximum resident set size/ {print $1}' "$WORK/t.txt")
        real=$(awk '/ real / {print $1}' "$WORK/t.txt")
        echo "$rss $real" >> "$WORK/m.txt"
        i=$((i + 1))
    done
    sort -n -k2 "$WORK/m.txt" | awk -v l="$label" -v n="$RUNS" 'NR==int((n+1)/2) {printf "%-34s %8.1f MB %8.3f s\n", l, $1/1048576, $2}'
}

export HOME="$WORK/home" CCSTATUSLINE_WIDTH=200
echo "transcript: $TRANSCRIPT ($(du -h "$TRANSCRIPT" | cut -f1))"
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
i=0
while [ $i -lt 60 ]; do
    ( /usr/bin/time -l "$BIN" --config "$WORK/config.json" < "$WORK/payload.json" > /dev/null 2> "$WORK/c$i.txt" ) &
    i=$((i + 1))
done
wait
end=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
cat "$WORK"/c*.txt | awk -v s="$start" -v e="$end" '/maximum resident set size/ {sum+=$1; if ($1>max) max=$1} END {printf "%-34s %8.1f MB max, %6.1f MB total, %6.3f s wall\n", "ccstatusline-rs 60 concurrent", max/1048576, sum/1048576, e-s}'
