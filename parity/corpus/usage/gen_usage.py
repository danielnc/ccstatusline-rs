#!/usr/bin/env python3
"""Generate the parity/corpus/usage/ cases for the usage widget group.

Usage: python3 parity/corpus/usage/gen_usage.py parity/corpus/usage
"""
import copy
import hashlib
import json
import os
import sys

OUT = sys.argv[1]
NOW_S = 1792584000  # 2026-10-21T12:00:00Z (harness default clock)

BASE_STDIN = {
    "hook_event_name": "Status",
    "session_id": "parity-session-0001",
    "cwd": "${CASE_DIR}/no-git",
    "model": {"id": "claude-opus-4-1-20250805", "display_name": "Opus 4.1"},
    "workspace": {"current_dir": "${CASE_DIR}/no-git", "project_dir": "${CASE_DIR}/no-git"},
    "version": "2.0.37",
    "context_window": {
        "context_window_size": 200000,
        "total_input_tokens": 45678,
        "total_output_tokens": 12345,
        "current_usage": {
            "input_tokens": 1234,
            "output_tokens": 567,
            "cache_creation_input_tokens": 8910,
            "cache_read_input_tokens": 34567,
        },
        "used_percentage": 22,
        "remaining_percentage": 78,
    },
}

RATE_LIMITS = {
    "five_hour": {"used_percentage": 42.5, "resets_at": NOW_S + 7987},
    "seven_day": {"used_percentage": 17, "resets_at": NOW_S + 279000},
    "seven_day_sonnet": {"used_percentage": 8, "resets_at": NOW_S + 279000},
    "seven_day_opus": {"used_percentage": 61.25, "resets_at": NOW_S + 279000},
}

cases = []


def sha16(s):
    return hashlib.sha256(s.encode()).hexdigest()[:16]


def add(name, widgets, stdin=None, rate_limits=RATE_LIMITS, files=None, env=None, config_extra=None, tags=None, **kw):
    st = copy.deepcopy(BASE_STDIN if stdin is None else stdin)
    if rate_limits is not None and stdin is None:
        st["rate_limits"] = copy.deepcopy(rate_limits)
    if isinstance(widgets, dict):
        widgets = [widgets]
    items = []
    for i, w in enumerate(widgets):
        it = {"id": str(i + 1)}
        it.update(w)
        items.append(it)
    config = {"version": 4, "colorLevel": 2, "lines": [items]}
    if config_extra:
        config.update(config_extra)
    case = {
        "name": f"usage/{name}",
        "fixtures": ["no-git"],
        "cwd": "no-git",
        "tags": sorted(set((tags or []) + [w["type"] for w in widgets] + ["usage-group"])),
        "config": config,
        "stdin": st,
    }
    if files:
        case["files"] = files
    if env:
        case["env"] = env
    case.update(kw)
    cases.append(case)


PERCENT = ["session-usage", "weekly-usage", "weekly-sonnet-usage", "weekly-opus-usage", "fable-weekly-usage"]
MODES = ["progress", "progress-short", "slider", "slider-only"]

# ---------------------------------------------------------------- percent widgets from rate_limits
for t in PERCENT:
    for m in MODES:
        add(f"{t}/{m}-cursor", {"type": t, "metadata": {"display": m, "cursor": "true"}})
        add(f"{t}/{m}-invert", {"type": t, "metadata": {"display": m, "invert": "true"}})
    add(f"{t}/time-invert", {"type": t, "metadata": {"invert": "true"}})
    add(f"{t}/compact-format", {"type": t, "numberFormat": {"style": "compact"}})
    add(f"{t}/whole-format", {"type": t, "numberFormat": {"style": "whole"}})
    add(f"{t}/decimals-3", {"type": t, "numberFormat": {"decimals": 3}})
    add(f"{t}/global-format", {"type": t}, config_extra={"numberFormat": {"percent": {"decimals": 0}}})
    add(f"{t}/raw-slider", {"type": t, "rawValue": True, "metadata": {"display": "slider"}})
    add(f"{t}/label", {"type": t, "metadata": {"label": "U "}})

odd = {
    "zero": {"five_hour": {"used_percentage": 0, "resets_at": NOW_S + 60}, "seven_day": {"used_percentage": 0, "resets_at": NOW_S + 60}},
    "over-100": {"five_hour": {"used_percentage": 140, "resets_at": NOW_S + 60}, "seven_day": {"used_percentage": 100.0, "resets_at": NOW_S - 60}},
    "negative": {"five_hour": {"used_percentage": -5, "resets_at": NOW_S + 18000}, "seven_day": {"used_percentage": -0.04}},
    "fractional": {"five_hour": {"used_percentage": 42.555, "resets_at": NOW_S + 1}, "seven_day": {"used_percentage": 99.95, "resets_at": NOW_S + 604800}},
    "string-numbers": {"five_hour": {"used_percentage": "17", "resets_at": str(NOW_S + 3600)}, "seven_day": {"used_percentage": " 3.5 ", "resets_at": None}},
    "nulls": {"five_hour": {"used_percentage": None, "resets_at": None}, "seven_day": {"used_percentage": 12, "resets_at": None}, "seven_day_sonnet": None, "seven_day_opus": None},
}
for k, rl in odd.items():
    add(f"odd/{k}-session", [{"type": "session-usage", "metadata": {"display": "progress", "cursor": "true"}}, {"type": "weekly-usage", "metadata": {"display": "slider", "cursor": "true"}}], rate_limits=rl)
    add(f"odd/{k}-timers", [{"type": "reset-timer"}, {"type": "weekly-reset-timer"}, {"type": "block-timer"}], rate_limits=rl)
    add(f"odd/{k}-models", [{"type": "weekly-sonnet-usage", "metadata": {"cursor": "true", "display": "slider"}}, {"type": "weekly-opus-usage"}, {"type": "fable-weekly-usage"}], rate_limits=rl)

# ---------------------------------------------------------------- timers from rate_limits
TIMERS = ["reset-timer", "weekly-reset-timer", "block-timer"]
for t in TIMERS:
    for m in MODES:
        add(f"{t}/{m}", {"type": t, "metadata": {"display": m}})
        add(f"{t}/{m}-invert", {"type": t, "metadata": {"display": m, "invert": "true"}})
    add(f"{t}/compact", {"type": t, "metadata": {"compact": "true"}})
    add(f"{t}/raw", {"type": t, "rawValue": True})
    add(f"{t}/decimals", {"type": t, "metadata": {"display": "progress"}, "numberFormat": {"decimals": 2}})

for t in ["reset-timer", "weekly-reset-timer"]:
    zones = ["America/New_York", "America/Los_Angeles", "Europe/Berlin", "Europe/London", "Asia/Kolkata", "Asia/Tokyo",
             "Australia/Sydney", "America/Sao_Paulo", "UTC", "Etc/UTC", "local", "Bogus/Zone", "america/chicago"]
    for z in zones:
        zn = z.replace("/", "-").lower()
        add(f"{t}/abs-{zn}", {"type": t, "metadata": {"absolute": "true", "timezone": z}})
        add(f"{t}/abs-{zn}-12h-compact", {"type": t, "metadata": {"absolute": "true", "timezone": z, "hour12": "true", "compact": "true"}})
        if t == "weekly-reset-timer":
            add(f"{t}/abs-{zn}-weekday", {"type": t, "metadata": {"absolute": "true", "timezone": z, "weekday": "true"}})
            add(f"{t}/abs-{zn}-weekday-12h", {"type": t, "metadata": {"absolute": "true", "timezone": z, "weekday": "true", "hour12": "true"}})
    add(f"{t}/abs-utc-12h", {"type": t, "metadata": {"absolute": "true", "hour12": "true"}})
    add(f"{t}/abs-locale-en-us", {"type": t, "metadata": {"absolute": "true", "timezone": "America/New_York", "locale": "en-US"}})
    add(f"{t}/abs-locale-unsupported", {"type": t, "metadata": {"absolute": "true", "timezone": "America/New_York", "locale": "xx-XX"}})
    add(f"{t}/abs-locale-invalid", {"type": t, "metadata": {"absolute": "true", "timezone": "Asia/Kolkata", "locale": "en_US"}})
    add(f"{t}/abs-tz-env", {"type": t, "metadata": {"absolute": "true", "timezone": "local"}}, env={"TZ": "America/Denver"})
    add(f"{t}/abs-progress-ignored", {"type": t, "metadata": {"absolute": "true", "display": "progress"}})
    add(f"{t}/abs-no-reset", {"type": t, "metadata": {"absolute": "true"}}, rate_limits={"five_hour": {"used_percentage": 1}, "seven_day": {"used_percentage": 1}})
add("weekly-reset-timer/hours-compact", {"type": "weekly-reset-timer", "metadata": {"hours": "true", "compact": "true"}})
add("weekly-reset-timer/hours-abs", {"type": "weekly-reset-timer", "metadata": {"hours": "true", "absolute": "true"}})
# DST boundary: reset right after the US fall-back
add("weekly-reset-timer/abs-dst-edge", {"type": "weekly-reset-timer", "metadata": {"absolute": "true", "timezone": "America/New_York"}},
    rate_limits={"seven_day": {"used_percentage": 5, "resets_at": 1793599200}, "five_hour": {"used_percentage": 1, "resets_at": NOW_S + 60}})

# ---------------------------------------------------------------- no rate_limits: API path, no network
CREDS = {"claudeAiOauth": {"accessToken": "parity-access-token", "refreshToken": "parity-refresh-token"}}
CREDS_HASH = sha16("parity-refresh-token")
ACCESS_HASH = sha16("parity-access-token")
ALL_USAGE = [{"type": t} for t in PERCENT] + [{"type": t} for t in TIMERS] + [
    {"type": "extra-usage-utilization"}, {"type": "extra-usage-remaining"}, {"type": "extra-usage-used"}, {"type": "extra-usage-daily-budget"}]

CACHE = {
    "sessionUsage": 33.3, "sessionResetAt": "2026-10-21T14:10:00.000Z", "weeklyUsage": 71,
    "weeklyResetAt": "2026-10-24T08:00:00+00:00", "weeklySonnetUsage": 4, "weeklyOpusUsage": 9.5,
    "weeklyOpusResetAt": "2026-10-25T08:00:00Z", "fableUsage": 2, "fableResetAt": "2026-10-26T08:00:00Z",
    "extraUsageEnabled": True, "extraUsageLimit": 500000, "extraUsageUsed": 106050, "extraUsageUtilization": 21.21,
    "extraUsageCurrency": "USD",
}


def cache_files(cache=CACHE, token_hash=None, creds=False, lock=None, extra=None):
    f = {}
    c = dict(cache or {})
    if token_hash is not None:
        c["tokenHash"] = token_hash
    if cache is not None:
        f[".cache/ccstatusline/usage.json"] = {"json": c}
    if creds:
        f[".claude/.credentials.json"] = {"json": CREDS if creds is True else creds}
    if lock is not None:
        f[".cache/ccstatusline/usage.lock"] = lock if isinstance(lock, str) else {"json": lock}
    if extra:
        f.update(extra)
    return f


add("api/no-creds-all", ALL_USAGE, rate_limits=None)
add("api/no-creds-hide", [dict(w, metadata={"hide": "no-data"}) for w in ALL_USAGE], rate_limits=None)
add("api/stale-cache-no-creds", ALL_USAGE, rate_limits=None, files=cache_files())
add("api/stale-cache-no-creds-modes", [dict(type=t, metadata={"display": "slider", "cursor": "true"}) for t in PERCENT], rate_limits=None, files=cache_files())
add("api/stale-cache-creds-match", ALL_USAGE, rate_limits=None, files=cache_files(token_hash=CREDS_HASH, creds=True))
add("api/stale-cache-creds-access-hash", ALL_USAGE, rate_limits=None, files=cache_files(token_hash=ACCESS_HASH, creds=True))
add("api/stale-cache-creds-mismatch", ALL_USAGE, rate_limits=None, files=cache_files(token_hash="0000000000000000", creds=True))
add("api/stale-cache-creds-nohash", ALL_USAGE, rate_limits=None, files=cache_files(creds=True))
add("api/creds-no-cache", ALL_USAGE, rate_limits=None, files=cache_files(cache=None, creds=True))
add("api/creds-access-only", [{"type": "session-usage"}], rate_limits=None,
    files=cache_files(token_hash=ACCESS_HASH, creds={"claudeAiOauth": {"accessToken": "parity-access-token"}}))
add("api/creds-invalid-json", [{"type": "session-usage"}], rate_limits=None, files={".claude/.credentials.json": "{nope", ".cache/ccstatusline/usage.json": {"json": CACHE}})
add("api/creds-empty-token", [{"type": "session-usage"}], rate_limits=None, files={".claude/.credentials.json": {"json": {"claudeAiOauth": {"accessToken": ""}}}})
add("api/lock-rate-limited", ALL_USAGE, rate_limits=None, files=cache_files(cache=None, creds=True, lock={"blockedUntil": NOW_S + 100, "error": "rate-limited"}))
add("api/lock-rate-limited-stale", ALL_USAGE, rate_limits=None, files=cache_files(token_hash=CREDS_HASH, creds=True, lock={"blockedUntil": NOW_S + 100, "error": "rate-limited"}))
add("api/lock-parse-error", [{"type": "weekly-usage"}], rate_limits=None, files=cache_files(cache=None, creds=True, lock={"blockedUntil": NOW_S + 10, "error": "parse-error"}))
add("api/lock-no-error-key", [{"type": "weekly-usage"}], rate_limits=None, files=cache_files(cache=None, creds=True, lock={"blockedUntil": NOW_S + 10}))
add("api/lock-expired", [{"type": "weekly-usage"}], rate_limits=None, files=cache_files(cache=None, creds=True, lock={"blockedUntil": NOW_S - 10, "error": "rate-limited"}))
add("api/lock-past-horizon", [{"type": "weekly-usage"}], rate_limits=None, files=cache_files(cache=None, creds=True, lock={"blockedUntil": NOW_S + 90000, "error": "rate-limited"}))
add("api/lock-legacy-text", [{"type": "weekly-usage"}], rate_limits=None, files=cache_files(cache=None, creds=True, lock="locked"))
add("api/lock-bad-error", [{"type": "weekly-usage"}], rate_limits=None, files=cache_files(cache=None, creds=True, lock={"blockedUntil": NOW_S + 10, "error": "boom"}))
add("api/cache-error-field", [{"type": "weekly-usage"}], rate_limits=None, files=cache_files(cache=dict(CACHE, error="timeout")))
add("api/cache-bad-shape", [{"type": "weekly-usage"}], rate_limits=None, files=cache_files(cache={"weeklyUsage": "71"}))
add("api/cache-array", [{"type": "weekly-usage"}], rate_limits=None, files={".cache/ccstatusline/usage.json": "[1,2]"})
add("api/cache-missing-fields", [{"type": "weekly-opus-usage"}, {"type": "extra-usage-used"}], rate_limits=None,
    files=cache_files(cache={"sessionUsage": 1, "weeklyUsage": 2}))
add("api/cache-fable-conclusive", [{"type": "fable-weekly-usage"}], rate_limits=None, files=cache_files(cache={"sessionUsage": 1}))
add("api/cache-extra-disabled", [{"type": "extra-usage-utilization"}, {"type": "extra-usage-remaining"}, {"type": "extra-usage-used"}, {"type": "extra-usage-daily-budget"}],
    rate_limits=None, files=cache_files(cache={"sessionUsage": 1, "weeklyUsage": 2, "extraUsageEnabled": False}))
add("api/cache-extra-disabled-hidden", [dict(type=t, metadata={"hide": "disabled"}) for t in ["extra-usage-utilization", "extra-usage-remaining", "extra-usage-used", "extra-usage-daily-budget"]],
    rate_limits=None, files=cache_files(cache={"sessionUsage": 1, "weeklyUsage": 2, "extraUsageEnabled": False}))
add("api/no-creds-lock-matching", [{"type": "session-usage"}], rate_limits=None,
    files=cache_files(cache=None, creds=True, extra={".cache/ccstatusline/usage-credentials.lock": {"json": {"blockedUntil": NOW_S + 10, "source": "[null,\"${HOME}/.claude\"]"}}}))
add("api/no-creds-lock-other-source", [{"type": "session-usage"}], rate_limits=None,
    files=cache_files(cache=None, creds=True, extra={".cache/ccstatusline/usage-credentials.lock": {"json": {"blockedUntil": NOW_S + 10, "source": "[null,\"/elsewhere\"]"}}}))
add("api/config-dir-creds", ALL_USAGE[:2], rate_limits=None, env={"CLAUDE_CONFIG_DIR": "${CASE_DIR}/cfg"},
    files={"${CASE_DIR}/cfg/.credentials.json": {"json": CREDS}, ".cache/ccstatusline/usage.json": {"json": dict(CACHE, tokenHash=CREDS_HASH)}})
add("api/config-dir-missing", ALL_USAGE[:2], rate_limits=None, env={"CLAUDE_CONFIG_DIR": "${CASE_DIR}/nope"}, files=cache_files(token_hash=CREDS_HASH, creds=True))
add("api/partial-rate-limits", [{"type": "session-usage"}, {"type": "weekly-usage"}, {"type": "weekly-opus-usage"}],
    rate_limits={"five_hour": {"used_percentage": 50, "resets_at": NOW_S + 100}}, files=cache_files())
add("api/partial-rate-limits-error", [{"type": "session-usage"}, {"type": "weekly-usage"}],
    rate_limits={"five_hour": {"used_percentage": 50, "resets_at": NOW_S + 100}})
add("api/timers-suppressed", [{"type": "reset-timer"}, {"type": "weekly-reset-timer"}],
    rate_limits={"five_hour": {"used_percentage": 50}, "seven_day": {"used_percentage": 5}})
add("api/cursor-needs-reset", [{"type": "session-usage", "metadata": {"cursor": "true", "display": "progress"}}, {"type": "weekly-sonnet-usage", "metadata": {"cursor": "true", "display": "slider"}}],
    rate_limits={"five_hour": {"used_percentage": 50}, "seven_day": {"used_percentage": 5, "resets_at": NOW_S + 5000}, "seven_day_sonnet": {"used_percentage": 3}}, files=cache_files())
add("api/rate-limits-override-cache", [{"type": "session-usage"}, {"type": "extra-usage-used"}],
    rate_limits={"five_hour": {"used_percentage": 5, "resets_at": NOW_S + 100}}, files=cache_files())

# ---------------------------------------------------------------- extra usage (from a stale cache, no creds)
for cur in ["USD", "EUR", "JPY", "GBP", "CHF", "brl", "XOF", "US", ""]:
    c = dict(CACHE, extraUsageCurrency=cur) if cur else {k: v for k, v in CACHE.items() if k != "extraUsageCurrency"}
    tag = cur.lower() or "none"
    add(f"extra/currency-{tag}", [{"type": "extra-usage-remaining"}, {"type": "extra-usage-used"}, {"type": "extra-usage-daily-budget"}], rate_limits=None, files=cache_files(cache=c))
for fmt_name, nf in [("compact", {"style": "compact"}), ("whole", {"style": "whole"}), ("dec0", {"decimals": 0}), ("dec4", {"decimals": 4})]:
    add(f"extra/format-{fmt_name}", [dict(type="extra-usage-remaining", numberFormat=nf), dict(type="extra-usage-used", numberFormat=nf),
                                     dict(type="extra-usage-daily-budget", numberFormat=nf), dict(type="extra-usage-utilization", numberFormat=nf)],
        rate_limits=None, files=cache_files())
add("extra/global-cost-format", [{"type": "extra-usage-used"}, {"type": "extra-usage-remaining"}], rate_limits=None, files=cache_files(),
    config_extra={"numberFormat": {"cost": {"style": "compact"}}})
add("extra/daily-weekdays", [{"type": "extra-usage-daily-budget", "metadata": {"weekdaysOnly": "true"}}], rate_limits=None, files=cache_files())
for d, ms in [("month-end", 1793447940000), ("weekend", 1792929600000), ("new-year-eve", 1798718400000)]:
    add(f"extra/daily-{d}", [{"type": "extra-usage-daily-budget"}, {"type": "extra-usage-daily-budget", "metadata": {"weekdaysOnly": "true"}}],
        rate_limits=None, files=cache_files(), nowMs=ms)
add("extra/over-limit", [{"type": "extra-usage-remaining"}, {"type": "extra-usage-utilization"}, {"type": "extra-usage-daily-budget"}], rate_limits=None,
    files=cache_files(cache=dict(CACHE, extraUsageUsed=600000, extraUsageUtilization=120)))
add("extra/utilization-null", [{"type": "extra-usage-utilization"}, {"type": "extra-usage-utilization", "metadata": {"display": "progress", "invert": "true"}}], rate_limits=None,
    files=cache_files(cache={k: v for k, v in CACHE.items() if k != "extraUsageUtilization"}))
add("extra/utilization-zero-limit", [{"type": "extra-usage-utilization"}], rate_limits=None,
    files=cache_files(cache={k: v for k, v in dict(CACHE, extraUsageLimit=0).items() if k != "extraUsageUtilization"}))
for m in MODES:
    add(f"extra/utilization-{m}", [{"type": "extra-usage-utilization", "metadata": {"display": m}}], rate_limits=None, files=cache_files())
add("extra/fractional-cents", [{"type": "extra-usage-used"}, {"type": "extra-usage-remaining"}], rate_limits=None,
    files=cache_files(cache=dict(CACHE, extraUsageUsed=100.5, extraUsageLimit=123456789)))
add("extra/raw", [dict(type=t, rawValue=True) for t in ["extra-usage-used", "extra-usage-remaining", "extra-usage-daily-budget", "extra-usage-utilization"]],
    rate_limits=None, files=cache_files())

# ---------------------------------------------------------------- block fallback (transcripts under ~/.claude/projects)
def jl(ts, sidechain=False, usage=True):
    o = {"timestamp": ts, "type": "assistant"}
    if usage:
        o["message"] = {"usage": {"input_tokens": 10, "output_tokens": 5}}
    if sidechain:
        o["isSidechain"] = True
    return json.dumps(o)


transcript = "\n".join([
    jl("2026-10-21T01:00:00.000Z"), jl("2026-10-21T09:12:00.000Z"), jl("2026-10-21T10:40:00Z"),
    jl("2026-10-21T11:55:00.000Z", sidechain=True), jl("2026-10-21T11:30:00.000Z", usage=False), "not json", jl("2026-10-21T11:20:00.000Z"),
])
BLOCK_FILES = {".claude/projects/-tmp-proj/session.jsonl": transcript}
for m in ["time"] + MODES:
    meta = {} if m == "time" else {"display": m}
    add(f"blocks/{m}", [{"type": "block-timer", "metadata": meta}, {"type": "reset-timer", "metadata": meta}],
        rate_limits={"seven_day": {"used_percentage": 1}}, files=BLOCK_FILES)
add("blocks/no-transcripts", [{"type": "block-timer"}, {"type": "reset-timer"}, {"type": "block-timer", "metadata": {"display": "slider"}},
                              {"type": "block-timer", "metadata": {"display": "progress"}}], rate_limits={"seven_day": {"used_percentage": 1}})
add("blocks/no-transcripts-hidden", [{"type": "block-timer", "metadata": {"hide": "no-data"}}, {"type": "reset-timer", "metadata": {"hide": "no-data"}}],
    rate_limits={"seven_day": {"used_percentage": 1}})
add("blocks/old-activity", [{"type": "block-timer"}, {"type": "reset-timer", "metadata": {"compact": "true"}}], rate_limits=None,
    files={".claude/projects/p/a.jsonl": jl("2026-10-20T01:00:00Z")})
add("blocks/gap", [{"type": "block-timer", "metadata": {"compact": "true"}}], rate_limits=None,
    files={".claude/projects/p/a.jsonl": "\n".join([jl("2026-10-21T00:10:00Z"), jl("2026-10-21T06:30:00Z"), jl("2026-10-21T11:59:00Z")]),
           ".claude/projects/p/sub/b.jsonl": "﻿" + jl("2026-10-21T07:05:00Z") + "\r\n",
           ".claude/projects/.hidden/c.jsonl": jl("2026-10-21T05:00:00Z")})
add("blocks/session-cursor", [{"type": "session-usage", "metadata": {"cursor": "true", "display": "progress"}}],
    rate_limits={"five_hour": {"used_percentage": 10}, "seven_day": {"used_percentage": 1}}, files=BLOCK_FILES)
add("blocks/config-dir", [{"type": "block-timer"}], rate_limits=None, env={"CLAUDE_CONFIG_DIR": "${CASE_DIR}/cfg"},
    files={"${CASE_DIR}/cfg/projects/x/s.jsonl": jl("2026-10-21T10:01:00Z")})

# ---------------------------------------------------------------- context bar
cw_variants = {
    "default": BASE_STDIN["context_window"],
    "no-window": None,
    "numeric-usage": {"context_window_size": 1000000, "current_usage": 250000},
    "pct-only": {"context_window_size": 200000, "used_percentage": 37.5},
    "over-100": {"context_window_size": 100000, "current_usage": {"input_tokens": 150000, "output_tokens": 0}},
    "zero-size": {"context_window_size": 0, "current_usage": {"input_tokens": 10}},
    "no-size": {"current_usage": {"input_tokens": 1500, "output_tokens": 10}},
    "tiny": {"context_window_size": 200000, "current_usage": {"input_tokens": 999, "cache_read_input_tokens": 0}},
    "big": {"context_window_size": 1000000, "current_usage": {"input_tokens": 999500}},
}
for k, cw in cw_variants.items():
    st = copy.deepcopy(BASE_STDIN)
    if cw is None:
        del st["context_window"]
    else:
        st["context_window"] = cw
    for m in ["", "progress", "slider", "slider-only", "bogus"]:
        meta = {"display": m} if m else {}
        add(f"context-bar/{k}{'-' + m if m else ''}", {"type": "context-bar", "metadata": meta}, stdin=st)
    add(f"context-bar/{k}-formats", {"type": "context-bar", "numberFormat": {"style": "compact", "decimals": 2}}, stdin=st)
add("context-bar/global-formats", {"type": "context-bar"}, config_extra={"numberFormat": {"token": {"decimals": 2}, "percent": {"decimals": 1}}})
add("context-bar/raw", {"type": "context-bar", "rawValue": True})

# ---------------------------------------------------------------- claude status
def status_cache(fetched_ms, indicator="minor", incidents=None, queried=True):
    return {".cache/ccstatusline/claude-status.json": {"json": {"fetchedAt": fetched_ms, "indicator": indicator, "incidents": incidents, "incidentsQueried": queried}}}


NOW_MS = NOW_S * 1000
INC = [
    {"impact": "minor", "startMs": NOW_MS - 47 * 3600000, "endMs": NOW_MS - 46 * 3600000},
    {"impact": "major", "startMs": NOW_MS - 20 * 3600000, "endMs": NOW_MS - 13 * 3600000},
    {"impact": "critical", "startMs": NOW_MS - 600000, "endMs": None},
]
for lvl in [0, 1, 2, 3]:
    add(f"claude-status/history-level-{lvl}", {"type": "claude-status", "metadata": {"history": "true"}}, files=status_cache(NOW_MS - 10000, incidents=INC), config_extra={"colorLevel": lvl})
    add(f"claude-status/no-data-history-level-{lvl}", {"type": "claude-status", "metadata": {"history": "true"}}, config_extra={"colorLevel": lvl})
for ind in ["none", "minor", "major", "critical", "maintenance", "weird"]:
    add(f"claude-status/indicator-{ind}", {"type": "claude-status"}, files=status_cache(NOW_MS - 10000, indicator=ind))
    add(f"claude-status/indicator-{ind}-history", {"type": "claude-status", "metadata": {"history": "true"}}, files=status_cache(NOW_MS - 10000, indicator=ind, incidents=[]))
add("claude-status/stale-cache", {"type": "claude-status"}, files=status_cache(NOW_MS - 400000, indicator="major"))
add("claude-status/stale-cache-history", {"type": "claude-status", "metadata": {"history": "true"}}, files=status_cache(NOW_MS - 400000, indicator="major", incidents=INC))
add("claude-status/not-queried-history", {"type": "claude-status", "metadata": {"history": "true"}}, files=status_cache(NOW_MS - 10000, queried=False))
add("claude-status/not-queried-plain", {"type": "claude-status"}, files=status_cache(NOW_MS - 10000, queried=False))
add("claude-status/null-indicator", {"type": "claude-status"}, files=status_cache(NOW_MS - 10000, indicator=None))
add("claude-status/bad-cache", {"type": "claude-status"}, files={".cache/ccstatusline/claude-status.json": "{\"fetchedAt\":\"x\"}"})
add("claude-status/failure-lock", {"type": "claude-status"}, files=dict(status_cache(NOW_MS - 400000, indicator="none"), **{".cache/ccstatusline/claude-status.lock": ""}))
add("claude-status/raw-history", {"type": "claude-status", "rawValue": True, "metadata": {"history": "true"}}, files=status_cache(NOW_MS - 10000, incidents=INC))
add("claude-status/powerline-history", [{"type": "claude-status", "metadata": {"history": "true"}}, {"type": "session-usage"}],
    files=status_cache(NOW_MS - 10000, incidents=INC), config_extra={"powerline": {"enabled": True}})
add("claude-status/two-widgets", [{"type": "claude-status"}, {"type": "claude-status", "metadata": {"history": "true"}}], files=status_cache(NOW_MS - 10000, incidents=INC))
add("claude-status/future-incident", {"type": "claude-status", "metadata": {"history": "true"}},
    files=status_cache(NOW_MS - 10000, incidents=[{"impact": "major", "startMs": NOW_MS + 1000, "endMs": None}, {"impact": "minor", "startMs": NOW_MS - 3600000 * 6, "endMs": NOW_MS - 3600000 * 6}]))

# ---------------------------------------------------------------- account email
add("claude-account-email/config-dir", {"type": "claude-account-email"}, env={"CLAUDE_CONFIG_DIR": "${CASE_DIR}/cfg"},
    files={"${CASE_DIR}/cfg/.claude.json": {"json": {"oauthAccount": {"emailAddress": "cfg@example.com"}}}, ".claude.json": {"json": {"oauthAccount": {"emailAddress": "home@example.com"}}}})
add("claude-account-email/config-dir-missing", {"type": "claude-account-email"}, env={"CLAUDE_CONFIG_DIR": "${CASE_DIR}/nope"},
    files={".claude.json": {"json": {"oauthAccount": {"emailAddress": "home@example.com"}}}})
add("claude-account-email/non-string", {"type": "claude-account-email"}, files={".claude.json": {"json": {"oauthAccount": {"emailAddress": 5}}}})
add("claude-account-email/raw-label", {"type": "claude-account-email", "rawValue": True}, files={".claude.json": {"json": {"oauthAccount": {"emailAddress": "x@y.z"}}}})

# ---------------------------------------------------------------- the live config shape
add("live/daniel-line", [{"type": "session-usage"}, {"type": "reset-timer"}, {"type": "weekly-usage"}, {"type": "weekly-reset-timer"}],
    config_extra={"colorLevel": 3, "flexMode": "full-minus-40", "powerline": {"enabled": True, "theme": "onedark", "separators": [""]}})
add("live/daniel-line-no-rate-limits", [{"type": "session-usage"}, {"type": "reset-timer"}, {"type": "weekly-usage"}, {"type": "weekly-reset-timer"}],
    rate_limits=None, files=cache_files(), config_extra={"colorLevel": 3, "powerline": {"enabled": True, "theme": "onedark"}})
add("live/minimalist", ALL_USAGE + [{"type": "context-bar"}, {"type": "claude-status"}], config_extra={"minimalistMode": True}, files=cache_files())
add("live/all-widgets", ALL_USAGE + [{"type": "context-bar"}, {"type": "claude-status"}, {"type": "claude-account-email"}])

for root, _, names in os.walk(OUT):
    for n in names:
        if n.endswith(".json"):
            os.remove(os.path.join(root, n))
for c in cases:
    path = os.path.join(OUT, c["name"].split("/", 1)[1] + ".json")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(c, f, indent=2, ensure_ascii=False)
        f.write("\n")
print(len(cases), "cases")
