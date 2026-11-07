#!/usr/bin/env python3
"""Merge lcov files from CI's harness parts into one, and gate on line coverage.

    lcov-merge.py --output lcov.info [--expect N] [--fail-under-lines PCT] FILE...

Each part runs a share of the harnesses against its own instrumented build of the same commit, so a line is
covered when any part covered it: hit counts are summed per source line and per function. Branch records are
dropped (CI does not collect them). Stdlib only.
"""
import argparse
import sys
from collections import defaultdict


def parse(path, lines, fn_lines, fn_hits):
    source = None
    with open(path, encoding="utf-8") as f:
        for raw in f:
            rec = raw.rstrip("\n")
            tag, _, rest = rec.partition(":")
            if tag == "SF":
                source = rest
                lines.setdefault(source, defaultdict(int))
                fn_lines.setdefault(source, {})
                fn_hits.setdefault(source, defaultdict(int))
            elif rec == "end_of_record":
                source = None
            elif source is None:
                continue
            elif tag == "DA":
                line, count = rest.split(",")[:2]
                lines[source][int(line)] += int(count)
            elif tag == "FN":
                line, name = rest.split(",", 1)
                fn_lines[source][name] = int(line)
            elif tag == "FNDA":
                count, name = rest.split(",", 1)
                fn_hits[source][name] += int(count)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="+")
    ap.add_argument("--output", required=True)
    ap.add_argument("--expect", type=int, help="fail unless exactly this many files are given")
    ap.add_argument("--fail-under-lines", type=float)
    args = ap.parse_args()
    if args.expect is not None and len(args.files) != args.expect:
        sys.exit(f"lcov-merge: expected {args.expect} lcov files, got {len(args.files)}: {' '.join(args.files)}")

    lines, fn_lines, fn_hits = {}, {}, {}
    for path in args.files:
        parse(path, lines, fn_lines, fn_hits)

    found = hit = 0
    with open(args.output, "w", encoding="utf-8") as out:
        for source in sorted(lines):
            out.write(f"SF:{source}\n")
            fns = fn_lines[source]
            for name, line in sorted(fns.items(), key=lambda kv: (kv[1], kv[0])):
                out.write(f"FN:{line},{name}\n")
            for name in sorted(fns, key=lambda n: (fns[n], n)):
                out.write(f"FNDA:{fn_hits[source][name]},{name}\n")
            out.write(f"FNF:{len(fns)}\nFNH:{sum(1 for n in fns if fn_hits[source][n] > 0)}\n")
            da = lines[source]
            for line in sorted(da):
                out.write(f"DA:{line},{da[line]}\n")
            lf, lh = len(da), sum(1 for c in da.values() if c > 0)
            out.write(f"LF:{lf}\nLH:{lh}\nend_of_record\n")
            found += lf
            hit += lh

    pct = 100.0 * hit / found if found else 0.0
    print(f"lines: {hit}/{found} covered ({pct:.2f}%) across {len(args.files)} files, {len(lines)} sources")
    if args.fail_under_lines is not None and pct < args.fail_under_lines:
        sys.exit(f"lcov-merge: line coverage {pct:.2f}% is under {args.fail_under_lines:g}%")


if __name__ == "__main__":
    main()
