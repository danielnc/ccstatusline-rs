#!/bin/sh
# Checks tools/bench.sh without a build: it runs a copy of the script from a
# checkout path holding a quote and a backslash, with stub renderers in place of
# upstream and ccstatusline-rs that log whether the payload they read is valid
# JSON carrying the right paths. Needs node, perl and BSD or GNU /usr/bin/time.
#   1. paths with quotes and backslashes still give a valid payload;
#   2. a failing render stops the run before its numbers are printed;
#   3. a time command that reports nothing stops the run with no numbers at all;
#   4. GNU time output is used only when both fields are numbers.
set -eu
here=$(dirname "$0")
here=$(CDPATH= cd -- "$here" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

root="$work/checkout \"q\" \\b"
transcript="$work/session \"q\" \\b.jsonl"
mkdir -p "$root/tools" "$root/target/release" "$root/parity/upstream/node_modules/ccstatusline/dist"
cp "$here/bench.sh" "$root/tools/bench.sh"
echo '{"type":"user"}' > "$transcript"
echo '{}' > "$work/config.json"

# The stub appends "ok" or "bad: <why>" to $BENCH_CHECK_LOG for every payload it reads.
cat > "$work/stub.js" <<'JS'
const fs = require('fs');
let input = '';
process.stdin.on('data', d => { input += d; }).on('end', () => {
    let verdict = 'ok';
    try {
        const p = JSON.parse(input);
        if (p.transcript_path !== process.env.BENCH_CHECK_TRANSCRIPT) verdict = `bad: transcript_path ${p.transcript_path}`;
        else if (p.cwd !== process.env.BENCH_CHECK_ROOT) verdict = `bad: cwd ${p.cwd}`;
    } catch (e) {
        verdict = `bad: ${e.message}`;
    }
    fs.appendFileSync(process.env.BENCH_CHECK_LOG, verdict + '\n');
    process.exit(verdict === 'ok' ? 0 : 1);
});
JS
cp "$work/stub.js" "$root/parity/upstream/node_modules/ccstatusline/dist/ccstatusline.js"
stub_ok() {
    printf '#!/bin/sh\nexec node "%s"\n' "$work/stub.js" > "$root/target/release/ccstatusline-rs"
    chmod +x "$root/target/release/ccstatusline-rs"
}
stub_failing() {
    printf '#!/bin/sh\nexit 1\n' > "$root/target/release/ccstatusline-rs"
    chmod +x "$root/target/release/ccstatusline-rs"
}

# run_bench [VAR=value...]: one run of the copy, stdout to $work/out, stderr to $work/err
run_bench() {
    env BENCH_CHECK_LOG="$work/log" BENCH_CHECK_TRANSCRIPT="$transcript" BENCH_CHECK_ROOT="$root" \
        CONFIG="$work/config.json" "$@" sh "$root/tools/bench.sh" "$transcript" 1 > "$work/out" 2> "$work/err"
}

# 1. Valid payload from awkward paths, and a full run.
stub_ok
: > "$work/log"
run_bench || { cat "$work/err" >&2; fail "bench.sh failed with working stubs"; }
grep -v '^ok$' "$work/log" && fail "a stub read a bad payload (above)"
[ "$(grep -c '^ok$' "$work/log")" -ge 63 ] || fail "expected at least 63 renders, got $(grep -c '^ok$' "$work/log")"
grep -q '^ccstatusline-rs 60 concurrent' "$work/out" || fail "no concurrent result printed"
grep -q '^machine: ' "$work/out" && grep -q '^date: ' "$work/out" || fail "results not labelled with machine and date"
grep -q "$work" "$work/out" && fail "the transcript's full path was printed"

# 2. A failing render stops the run, and none of its numbers are printed.
stub_failing
: > "$work/log"
run_bench && fail "bench.sh succeeded with a failing ccstatusline-rs"
grep -q '^ccstatusline-rs' "$work/out" && fail "printed numbers for a failing render"
grep -q 'render failed' "$work/err" || fail "no render failure reported"

# 3. A time command that runs the command but reports nothing stops the run before any numbers.
stub_ok
printf '#!/bin/sh\nshift\nexec "$@"\n' > "$work/notime"
chmod +x "$work/notime"
: > "$work/log"
run_bench TIME_CMD="$work/notime" && fail "bench.sh succeeded with a time command that reports nothing"
grep -q ' MB ' "$work/out" && fail "printed numbers without measurements"
grep -q 'reported no peak RSS' "$work/err" || fail "no measurement failure reported"

# 4. A GNU-style time (-f, no -l) that writes $FAKE_TIME_OUTPUT as its measurement.
cat > "$work/gnutime" <<'SH'
#!/bin/sh
[ "$1" = -f ] || exit 2
shift 2
out=/dev/stderr
if [ "$1" = -o ]; then out=$2; shift 2; fi
status=0
"$@" || status=$?
printf '%s\n' "$FAKE_TIME_OUTPUT" > "$out"
exit "$status"
SH
chmod +x "$work/gnutime"
run_bench TIME_CMD="$work/gnutime" FAKE_TIME_OUTPUT="1000 0.10" || { cat "$work/err" >&2; fail "bench.sh failed with GNU time"; }
grep -q '^ccstatusline-rs warm .* 1\.0 MB ' "$work/out" || fail "GNU time's KB not converted to MB: $(cat "$work/out")"
run_bench TIME_CMD="$work/gnutime" FAKE_TIME_OUTPUT="invalid 0.10" && fail "bench.sh accepted a non-numeric RSS"
grep -q ' MB ' "$work/out" && fail "printed numbers for a non-numeric RSS"
run_bench TIME_CMD="$work/gnutime" FAKE_TIME_OUTPUT="1000 1.2.3" && fail "bench.sh accepted a malformed wall time"
grep -q ' MB ' "$work/out" && fail "printed numbers for a malformed wall time"
grep -q 'reported no wall time' "$work/err" || fail "no wall-time failure reported"

echo "ok   tools/bench.sh: JSON-safe payload, failures stop the run, results labelled"
