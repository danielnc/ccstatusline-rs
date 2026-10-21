#!/usr/bin/env python3
"""Generate parity/corpus/misc/ cases for the misc widget group.

Usage: python3 parity/corpus/misc/gen_misc.py parity/corpus/misc  (rewrites the *.json cases)
"""
import copy
import json
import os
import sys

OUT = sys.argv[1]

BASE_STDIN = {
    "hook_event_name": "Status",
    "session_id": "parity-session-0001",
    "cwd": "${CASE_DIR}/no-git",
    "model": {"id": "claude-opus-4-1-20250805", "display_name": "Opus 4.1"},
    "workspace": {"current_dir": "${CASE_DIR}/no-git", "project_dir": "${CASE_DIR}/no-git"},
    "version": "2.0.37",
    "output_style": {"name": "Explanatory"},
    "cost": {
        "total_cost_usd": 1.2345,
        "total_duration_ms": 3723000,
        "total_api_duration_ms": 1234567,
        "total_lines_added": 156,
        "total_lines_removed": 23,
    },
}

cases = []


def case(widget, name, items, stdin=None, stdin_patch=None, drop=(), tags=(), **extra):
    if isinstance(items, dict):
        items = [items]
    lines = [[dict({"id": str(i + 1)}, **it) for i, it in enumerate(items)]]
    cfg = {"version": 4, "colorLevel": 0, "lines": lines}
    cfg.update(extra.pop("config_extra", {}))
    if stdin is None:
        stdin = copy.deepcopy(BASE_STDIN)
        for k in drop:
            stdin.pop(k, None)
        if stdin_patch:
            stdin.update(stdin_patch)
    c = {
        "name": f"misc/{widget}/{name}",
        "tags": [widget, "misc"] + list(tags),
        "fixtures": ["no-git"],
        "cwd": "no-git",
        "config": cfg,
        "stdin": stdin,
    }
    c.update(extra)
    cases.append(c)


def w(type_, **kw):
    return dict({"type": type_}, **kw)


# ------------------------------------------------------------ output-style
for nm, patch, meta in [
    ("named", {}, None),
    ("default-shown", {"output_style": {"name": "default"}}, None),
    ("default-hidden", {"output_style": {"name": "default"}}, {"hide": "default-value"}),
    ("other-hidden-key", {"output_style": {"name": "Learning"}}, {"hide": "default-value"}),
    ("hide-list-other", {"output_style": {"name": "default"}}, {"hide": " zero , other "}),
    ("empty-name", {"output_style": {"name": ""}}, None),
    ("null-style", {"output_style": None}, None),
    ("no-name", {"output_style": {}}, None),
    ("label", {}, {"label": "✎ "}),
    ("empty-label", {}, {"label": ""}),
]:
    it = w("output-style")
    if meta:
        it["metadata"] = meta
    case("output-style", nm, it, stdin_patch=patch)
case("output-style", "raw", w("output-style", rawValue=True))
case("output-style", "missing", w("output-style"), drop=["output_style"])

# ------------------------------------------------------------ session-cost
for nm, cost, it_extra, cfg in [
    ("basic", 1.2345, {}, {}),
    ("string-number", " 3.5 ", {}, {}),
    ("zero", 0, {}, {}),
    ("zero-hidden", 0, {"metadata": {"hide": "zero"}}, {}),
    ("below-cent-hidden", 0.0049, {"metadata": {"hide": "zero"}}, {}),
    ("cent-hidden-key", 0.005, {"metadata": {"hide": "zero"}}, {}),
    ("negative-hidden", -0.001, {"metadata": {"hide": "zero"}}, {}),
    ("large", 123456.789, {}, {}),
    ("huge", 1e21, {}, {}),
    ("tiny", 1e-7, {}, {}),
    ("compact", 2.5, {"numberFormat": {"style": "compact"}}, {}),
    ("whole", 2.5, {"numberFormat": {"style": "whole"}}, {}),
    ("decimals-4", 2.5, {"numberFormat": {"decimals": 4}}, {}),
    ("decimals-0", 2.5, {"numberFormat": {"decimals": 0}}, {}),
    ("global-format", 2.555, {"numberFormat": {"decimals": 4}}, {"numberFormat": {"cost": {"decimals": 1}}}),
    ("raw", 9.99, {"rawValue": True}, {}),
    ("label", 9.99, {"metadata": {"label": "💰"}}, {}),
]:
    c = copy.deepcopy(BASE_STDIN)
    c["cost"]["total_cost_usd"] = cost
    case("session-cost", nm, w("session-cost", **it_extra), stdin=c, config_extra=cfg)
case("session-cost", "no-cost", w("session-cost"), drop=["cost"])
case("session-cost", "null-cost", w("session-cost"), stdin_patch={"cost": None})
case("session-cost", "cost-without-total", w("session-cost"), stdin_patch={"cost": {"total_duration_ms": 5}})

# ------------------------------------------------------- session-cost-rate
for nm, cost, it_extra in [
    ("active", {"total_cost_usd": 1.5, "total_api_duration_ms": 1800000}, {}),
    ("clock", {"total_cost_usd": 1.5, "total_duration_ms": 7200000}, {"metadata": {"clockTime": "true"}}),
    ("clock-false", {"total_cost_usd": 1.5, "total_api_duration_ms": 900000, "total_duration_ms": 7200000}, {"metadata": {"clockTime": "false"}}),
    ("exactly-minute", {"total_cost_usd": 0.1, "total_api_duration_ms": 60000}, {}),
    ("under-minute", {"total_cost_usd": 0.1, "total_api_duration_ms": 59999}, {}),
    ("no-duration", {"total_cost_usd": 0.1}, {}),
    ("clock-missing", {"total_cost_usd": 0.1, "total_api_duration_ms": 600000}, {"metadata": {"clockTime": "true"}}),
    ("no-total", {"total_api_duration_ms": 600000}, {}),
    ("zero-cost", {"total_cost_usd": 0, "total_api_duration_ms": 600000}, {}),
    ("string-values", {"total_cost_usd": "2", "total_api_duration_ms": "3600000"}, {}),
    ("odd-ratio", {"total_cost_usd": 1.2345, "total_api_duration_ms": 1234567}, {}),
    ("compact", {"total_cost_usd": 1, "total_api_duration_ms": 3600000}, {"numberFormat": {"style": "compact"}}),
    ("raw", {"total_cost_usd": 1, "total_api_duration_ms": 1800000}, {"rawValue": True}),
    ("label", {"total_cost_usd": 1, "total_api_duration_ms": 1800000}, {"metadata": {"label": "⏱ "}}),
]:
    case("session-cost-rate", nm, w("session-cost-rate", **it_extra), stdin_patch={"cost": cost})

# ---------------------------------------------------------- terminal-width
case("terminal-width", "with-width", w("terminal-width"), env={"CCSTATUSLINE_WIDTH": "123"})
case("terminal-width", "no-width", w("terminal-width"))
case("terminal-width", "raw", w("terminal-width", rawValue=True), env={"CCSTATUSLINE_WIDTH": "80"})
case("terminal-width", "label", w("terminal-width", metadata={"label": "cols="}), env={"CCSTATUSLINE_WIDTH": "80"})
case("terminal-width", "bad-env", w("terminal-width"), env={"CCSTATUSLINE_WIDTH": "abc"})
case("terminal-width", "float-env", w("terminal-width"), env={"CCSTATUSLINE_WIDTH": "99.7"})

# ----------------------------------------------------------------- version
case("version", "basic", w("version"))
case("version", "raw", w("version", rawValue=True))
case("version", "empty", w("version"), stdin_patch={"version": ""})
case("version", "null", w("version"), stdin_patch={"version": None})
case("version", "missing", w("version"), drop=["version"])
case("version", "label-ignored", w("version", metadata={"label": "CC "}))

# ------------------------------------------------------ custom-text/symbol
case("custom-text", "plain", w("custom-text", customText="hello world"))
case("custom-text", "missing", [w("custom-text"), w("version")])
case("custom-text", "empty", [w("custom-text", customText=""), w("version")])
case("custom-text", "emoji", w("custom-text", customText="🚀 ship it ✨"))
case("custom-text", "ansi-in-text", w("custom-text", customText="\u001b[31mred\u001b[0m and \u0007bell"))
case("custom-text", "raw-ignored", w("custom-text", customText="x", rawValue=True))
case("custom-text", "merge-target-hidden", [
    w("custom-text", customText="v:", merge="no-padding", metadata={"hide": "merge-target-hidden"}),
    w("version"),
], drop=["version"])
case("custom-text", "merge-target-shown", [
    w("custom-text", customText="v:", merge="no-padding", metadata={"hide": "merge-target-hidden"}),
    w("version"),
])
case("custom-symbol", "plain", w("custom-symbol", customSymbol="★"))
case("custom-symbol", "missing", [w("custom-symbol"), w("version")])
case("custom-symbol", "flag", w("custom-symbol", customSymbol="🇧🇷"))
case("custom-symbol", "merge-target-hidden", [
    w("custom-symbol", customSymbol="★", merge=True, metadata={"hide": "merge-target-hidden"}),
    w("output-style"),
], drop=["output_style"])

# ------------------------------------------------------- claude-session-id
case("claude-session-id", "basic", w("claude-session-id"))
case("claude-session-id", "raw", w("claude-session-id", rawValue=True))
case("claude-session-id", "empty", w("claude-session-id"), stdin_patch={"session_id": ""})
case("claude-session-id", "missing", w("claude-session-id"), drop=["session_id"])
case("claude-session-id", "label", w("claude-session-id", metadata={"label": "#"}))

# ----------------------------------------------------- current-working-dir
CWD = "current-working-dir"
for nm, cwd, meta, extra in [
    ("plain", None, None, {}),
    ("segments-2", "/a/b/c/d/e", {"segments": "2"}, {}),
    ("segments-more-than-parts", "/a/b", {"segments": "5"}, {}),
    ("segments-zero", "/a/b/c", {"segments": "0"}, {}),
    ("segments-negative", "/a/b/c", {"segments": "-1"}, {}),
    ("segments-garbage", "/a/b/c", {"segments": "abc"}, {}),
    ("segments-trailing-garbage", "/a/b/c/d", {"segments": "2x"}, {}),
    ("segments-empty", "/a/b/c/d", {"segments": ""}, {}),
    ("segments-backslash", "C:\\Users\\me\\proj\\src", {"segments": "2"}, {}),
    ("segments-mixed-sep", "C:\\Users/me\\proj", {"segments": "1"}, {}),
    ("segments-trailing-slash", "/a/b/c/", {"segments": "1"}, {}),
    ("fish", "/usr/local/share/.config/project", {"fishStyle": "true"}, {}),
    ("fish-root", "/", {"fishStyle": "true"}, {}),
    ("fish-relative", "rel/one/two", {"fishStyle": "true"}, {}),
    ("fish-backslash", "C:\\Users\\me\\.hidden\\proj", {"fishStyle": "true"}, {}),
    ("fish-unicode", "/x/éclair/ünïcode/end", {"fishStyle": "true"}, {}),
    ("fish-astral", "/x/😀dir/.😀/end", {"fishStyle": "true"}, {}),
    ("fish-home", "${HOME}/Documents/Projects/x", {"fishStyle": "true"}, {}),
    ("fish-dot-only", "/a/./b/end", {"fishStyle": "true"}, {}),
    ("fish-wins-over-segments", "/a/b/c/d", {"fishStyle": "true", "segments": "1"}, {}),
    ("home", "${HOME}/projects/app", {"abbreviateHome": "true"}, {}),
    ("home-exact", "${HOME}", {"abbreviateHome": "true"}, {}),
    ("home-not-boundary", "${HOME}foo/bar", {"abbreviateHome": "true"}, {}),
    ("home-outside", "/opt/thing", {"abbreviateHome": "true"}, {}),
    ("home-segments", "${HOME}/a/b/c", {"abbreviateHome": "true", "segments": "2"}, {}),
    ("home-segments-fit", "${HOME}/a/b", {"abbreviateHome": "true", "segments": "2"}, {}),
    ("symbol", None, None, {"character": "📁"}),
    ("symbol-empty", None, None, {"character": ""}),
    ("raw", None, None, {"rawValue": True}),
    ("raw-symbol", "/a/b", None, {"rawValue": True, "character": ">"}),
    ("label", None, {"label": "dir "}, {}),
    ("empty-cwd", "", None, {}),
]:
    it = w(CWD, **extra)
    if meta:
        it["metadata"] = meta
    patch = {} if cwd is None else {"cwd": cwd}
    case(CWD, nm, it, stdin_patch=patch)
case(CWD, "missing-cwd", w(CWD), drop=["cwd"])

# -------------------------------------------------------------------- link
LINKS = [
    ("url-only", {"url": "https://example.com/docs"}),
    ("text-only", {"text": "Docs"}),
    ("nothing", None),
    ("trimmed", {"url": "  https://example.com/a  ", "text": "  Read  "}),
    ("blank-text", {"url": "https://example.com", "text": "   "}),
    ("ftp", {"url": "ftp://example.com/file", "text": "File"}),
    ("not-a-url", {"url": "example.com", "text": "Ex"}),
    ("no-host", {"url": "https://", "text": "Bad"}),
    ("no-slashes", {"url": "http:example.com/x", "text": "NS"}),
    ("uppercase-safe", {"url": "HTTPS://Example.COM/A", "text": "Up"}),
    ("space-in-path", {"url": "https://example.com/my page?q=a b#frag ment", "text": "Sp"}),
    ("space-in-host", {"url": "https://exa mple.com/", "text": "Bad host"}),
    ("tab-in-url", {"url": "https://example.com/a\tb", "text": "Tab"}),
    ("control-in-url", {"url": "https://example.com/a\u0001b", "text": "Ctl"}),
    ("unicode-path-space", {"url": "https://example.com/ünï cødé", "text": "Uni"}),
    ("unicode-host-space", {"url": "https://bücher.de/a b", "text": "IDN"}),
    ("ipv6", {"url": "http://[::1]:8080/x y", "text": "v6"}),
    ("ipv4-hex", {"url": "http://0x7f.1/a b", "text": "v4"}),
    ("bad-ipv4", {"url": "http://1.2.3.999/", "text": "v4bad"}),
    ("bad-port", {"url": "http://example.com:99999/", "text": "port"}),
    ("default-port-space", {"url": "https://example.com:443/a b", "text": "dp"}),
    ("userinfo-space", {"url": "https://us er:pa ss@example.com/x y", "text": "ui"}),
    ("dot-segments-space", {"url": "https://example.com/a/./b/../c d", "text": "dots"}),
    ("backslashes-space", {"url": "https:\\\\example.com\\a\\b c", "text": "bs"}),
    ("query-quote", {"url": "https://example.com/?a='b c'&d=\"e\"", "text": "q"}),
    ("caret-path", {"url": "https://example.com/a^b c{d}`e", "text": "caret"}),
    ("del-char", {"url": "https://example.com/a\u007fb", "text": "del"}),
    ("c1-char", {"url": "https://example.com/a\u0085b", "text": "c1"}),
    ("percent-host", {"url": "https://ex%41mple.com/ x", "text": "pct"}),
    ("javascript", {"url": "javascript:alert(1)", "text": "js"}),
]
for nm, meta in LINKS:
    it = w("link")
    if meta is not None:
        it["metadata"] = meta
    case("link", nm, it)
case("link", "raw-url", w("link", rawValue=True, metadata={"url": "https://example.com/a b"}))
case("link", "raw-nothing", w("link", rawValue=True))

# ----------------------------------------------------------- free-memory
FM = "free-memory"
case(FM, "default-env", w(FM))
case(FM, "raw", w(FM, rawValue=True))
case(FM, "label", w(FM, metadata={"label": "RAM "}))
case(FM, "megabytes", w(FM), env={"CCSL_TEST_MEM_TOTAL_BYTES": "900000000", "CCSL_TEST_MEM_FREE_BYTES": "100000000"})
case(FM, "kilobytes", w(FM), env={"CCSL_TEST_MEM_TOTAL_BYTES": "900000", "CCSL_TEST_MEM_FREE_BYTES": "1000"})
case(FM, "bytes", w(FM), env={"CCSL_TEST_MEM_TOTAL_BYTES": "1000", "CCSL_TEST_MEM_FREE_BYTES": "999"})
case(FM, "nan-total", w(FM), env={"CCSL_TEST_MEM_TOTAL_BYTES": "lots", "CCSL_TEST_MEM_FREE_BYTES": "5"})
case(FM, "negative-used", w(FM), env={"CCSL_TEST_MEM_TOTAL_BYTES": "1024", "CCSL_TEST_MEM_FREE_BYTES": "4096"})
case(FM, "fractional", w(FM), env={"CCSL_TEST_MEM_TOTAL_BYTES": "1536.5", "CCSL_TEST_MEM_FREE_BYTES": "0.25"})
case(FM, "huge", w(FM), env={"CCSL_TEST_MEM_TOTAL_BYTES": "1e30", "CCSL_TEST_MEM_FREE_BYTES": "1e29"})
case(FM, "compact", w(FM, numberFormat={"style": "compact"}), env={"CCSL_TEST_MEM_TOTAL_BYTES": "17179869184", "CCSL_TEST_MEM_FREE_BYTES": "0"})
case(FM, "decimals", w(FM, numberFormat={"decimals": 3}))
case(FM, "whole", w(FM, numberFormat={"style": "whole"}))
case(FM, "global-memory-format", w(FM), config_extra={"numberFormat": {"memory": {"decimals": 2}}})

# ------------------------------------------------------------- vim-mode
for nm, vim, it_extra in [
    ("normal", {"mode": "NORMAL"}, {}),
    ("insert-letter", {"mode": "INSERT"}, {"metadata": {"format": "letter"}}),
    ("visual-icon-letter", {"mode": "VISUAL"}, {"metadata": {"format": "icon-letter"}}),
    ("visual-line-word", {"mode": "VISUAL LINE"}, {"metadata": {"format": "word"}}),
    ("empty-mode", {"mode": ""}, {}),
    ("empty-mode-letter", {"mode": ""}, {"metadata": {"format": "letter"}}),
    ("lowercase", {"mode": "replace"}, {}),
    ("astral-first", {"mode": "😀mode"}, {}),
    ("nerd-icon", {"mode": "NORMAL"}, {"metadata": {"format": "icon", "nerdFont": "true"}}),
    ("nerd-letter-ignored", {"mode": "NORMAL"}, {"metadata": {"format": "letter", "nerdFont": "true"}}),
    ("unknown-format", {"mode": "INSERT"}, {"metadata": {"format": "bogus"}}),
    ("no-mode", {}, {}),
    ("null-vim", None, {}),
    ("raw-ignored", {"mode": "NORMAL"}, {"rawValue": True}),
]:
    case("vim-mode", nm, w("vim-mode", **it_extra), stdin_patch={"vim": vim})
case("vim-mode", "missing", w("vim-mode"))

# ----------------------------------------------- sandbox / voice layers
PROJ_LOCAL = "${CASE_DIR}/no-git/.claude/settings.local.json"
PROJ = "${CASE_DIR}/no-git/.claude/settings.json"
USER_LOCAL = ".claude/settings.local.json"
USER = ".claude/settings.json"


def layer_cases(widget, key, formats):
    for nm, files in [
        ("no-files", {}),
        ("user-true", {USER: {"json": {key: {"enabled": True}}}}),
        ("user-false", {USER: {"json": {key: {"enabled": False}}}}),
        ("user-local-over-user", {USER: {"json": {key: {"enabled": False}}}, USER_LOCAL: {"json": {key: {"enabled": True}}}}),
        ("project-over-user", {USER: {"json": {key: {"enabled": True}}}, PROJ: {"json": {key: {"enabled": False}}}}),
        ("project-local-top", {PROJ: {"json": {key: {"enabled": False}}}, PROJ_LOCAL: {"json": {key: {"enabled": True}}}}),
        ("file-without-key", {USER: {"json": {"other": 1}}}),
        ("key-null", {PROJ: {"json": {key: None}}, USER: {"json": {key: {"enabled": True}}}}),
        ("enabled-not-bool", {PROJ: {"json": {key: {"enabled": "yes"}}}, USER: {"json": {key: {"enabled": True}}}}),
        ("key-is-array", {PROJ: {"json": {key: [True]}}, USER: {"json": {key: {"enabled": True}}}}),
        ("enabled-missing", {PROJ: {"json": {key: {"mode": "x"}}}}),
        ("malformed-json", {PROJ: "{ nope", USER: {"json": {key: {"enabled": True}}}}),
        ("malformed-only", {PROJ: "{ nope"}),
        ("json-null-file", {PROJ: "null"}),
        ("json-array-file", {PROJ: "[1,2]"}),
        ("bom-file", {PROJ: "\ufeff{\"" + key + "\":{\"enabled\":true}}"}),
        ("empty-file", {PROJ: ""}),
        ("extra-keys", {PROJ: {"json": {key: {"enabled": True, "extra": 1}}}}),
    ]:
        case(widget, nm, w(widget), files=files)
    # a directory where a settings file should be: EISDIR counts as existing
    case(widget, "settings-is-directory", w(widget), files={PROJ + "/keep": "x"})
    # project_dir wins over cwd for the project layer
    case(widget, "project-dir-preferred", w(widget),
         files={"${CASE_DIR}/proj/.claude/settings.json": {"json": {key: {"enabled": True}}}},
         stdin_patch={"workspace": {"current_dir": "${CASE_DIR}/no-git", "project_dir": "${CASE_DIR}/proj"}})
    case(widget, "blank-project-dir", w(widget), files={PROJ: {"json": {key: {"enabled": True}}}},
         stdin_patch={"workspace": {"current_dir": "${CASE_DIR}/elsewhere", "project_dir": "   "}})
    case(widget, "current-dir-last", w(widget),
         files={"${CASE_DIR}/cur/.claude/settings.json": {"json": {key: {"enabled": True}}}},
         stdin_patch={"workspace": {"current_dir": "${CASE_DIR}/cur"}, "cwd": ""})
    case(widget, "process-cwd-fallback", w(widget), files={PROJ: {"json": {key: {"enabled": True}}}},
         drop=["workspace", "cwd"])
    case(widget, "dotdot-cwd", w(widget), files={PROJ: {"json": {key: {"enabled": True}}}},
         stdin_patch={"workspace": {"project_dir": "${CASE_DIR}/no-git/sub/.."}})
    case(widget, "config-dir-env", w(widget),
         files={"${CASE_DIR}/cfg/settings.json": {"json": {key: {"enabled": True}}}},
         env={"CLAUDE_CONFIG_DIR": "${CASE_DIR}/cfg"})
    for fmt in formats:
        for nerd in (False, True):
            for raw in (False, True):
                it = w(widget, metadata={"format": fmt})
                if nerd:
                    it["metadata"]["nerdFont"] = "true"
                if raw:
                    it["rawValue"] = True
                nm = f"fmt-{fmt}{'-nerd' if nerd else ''}{'-raw' if raw else ''}"
                case(widget, nm + "-on", it, files={USER: {"json": {key: {"enabled": True}}}})
                case(widget, nm + "-off", copy.deepcopy(it), files={USER: {"json": {key: {"enabled": False}}}})


layer_cases("sandbox-status", "sandbox", ["glyph", "text", "word"])
layer_cases("voice-status", "voice", ["icon", "icon-text", "text", "word"])

# --------------------------------------------------- remote-control-status
RC = "remote-control-status"
SESS = ".claude/sessions/"
for nm, files, patch in [
    ("no-dir", {}, {}),
    ("empty-dir", {SESS + "keep.txt": "x"}, {}),
    ("attached", {SESS + "123.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": "br-1"}}}, {}),
    ("detached-null", {SESS + "123.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": None}}}, {}),
    ("detached-empty", {SESS + "123.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": ""}}}, {}),
    ("detached-missing", {SESS + "123.json": {"json": {"sessionId": "parity-session-0001", "pid": 123}}}, {}),
    ("other-session", {SESS + "123.json": {"json": {"sessionId": "someone-else", "bridgeSessionId": "x"}}}, {}),
    ("first-match-sorted", {
        SESS + "200.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": None}},
        SESS + "1000.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": "b"}},
    }, {}),
    ("invalid-json-skipped", {
        SESS + "1.json": "{ broken",
        SESS + "2.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": "b"}},
    }, {}),
    ("bad-types-skipped", {
        SESS + "1.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": 5}},
        SESS + "2.json": {"json": {"sessionId": 7}},
        SESS + "3.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": None}},
    }, {}),
    ("array-file-skipped", {SESS + "1.json": "[]", SESS + "2.json": "null"}, {}),
    ("non-json-ext", {SESS + "1.txt": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": "b"}}}, {}),
    ("json-dir", {SESS + "d.json/x": "y", SESS + "e.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": "b"}}}, {}),
    ("no-session-id", {SESS + "1.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": "b"}}}, {"session_id": ""}),
]:
    case(RC, nm, w(RC), files=files, stdin_patch=patch)
case(RC, "config-dir-env", w(RC, metadata={"format": "word"}),
     files={"${CASE_DIR}/cfg/sessions/9.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": "b"}}},
     env={"CLAUDE_CONFIG_DIR": "${CASE_DIR}/cfg"})
for fmt in ["icon", "icon-text", "text", "word", "label-check", "label-mark", "bogus"]:
    for nerd in (False, True):
        for raw in (False, True):
            it = w(RC, metadata={"format": fmt})
            if nerd:
                it["metadata"]["nerdFont"] = "true"
            if raw:
                it["rawValue"] = True
            nm = f"fmt-{fmt}{'-nerd' if nerd else ''}{'-raw' if raw else ''}"
            for on in (True, False):
                case(RC, nm + ("-on" if on else "-off"), copy.deepcopy(it),
                     files={SESS + "1.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": "b" if on else None}}})

# ------------------------------------------------------- custom-command
CC = "custom-command"


def cc(name, cmd, files=None, items_extra=None, more_items=(), **kw):
    it = w(CC, commandPath=cmd)
    if items_extra:
        it.update(items_extra)
    case(CC, name, [it] + list(more_items), files=files or {}, tags=["process"], **kw)


cc("echo", "echo hello")
cc("printf-no-newline", "printf 'a  b'")
cc("whitespace-only", "printf '  \\n\\t '", more_items=[w("version")])
cc("multiline", "printf 'one\\ntwo\\n'")
cc("cr-lf", "printf 'one\\r\\ntwo'")
cc("exit-1", "echo out; exit 1")
cc("exit-255", "exit 255")
cc("not-found", "definitely-not-a-command-xyz")
cc("signal-term", "kill -TERM $$")
cc("signal-kill", "kill -KILL $$")
cc("signal-int", "kill -INT $$")
cc("stderr-ignored", "echo err >&2; echo ok")
cc("timeout", "sleep 3; echo late", items_extra={"timeout": 200})
cc("timeout-zero-means-none", "sleep 0.2; echo done", items_extra={"timeout": 0})
cc("timeout-negative", "echo neg", items_extra={"timeout": -5})
cc("background-holds-stdout", "echo bg; (sleep 2 &); exit 0", items_extra={"timeout": 300})
cc("background-exit-code", "echo bg; (sleep 2 &); exit 4", items_extra={"timeout": 300})
cc("stdin-json", "cat", env={"CCSTATUSLINE_WIDTH": "5000"})
cc("stdin-json-no-width", "cat", config_extra={"flexMode": "full"})
cc("stdin-shape", "cat", env={"CCSTATUSLINE_WIDTH": "5000"}, stdin={
    "zzz": 1.50, "cost": {"extra": 1, "total_lines_removed": "7", "total_cost_usd": " 1.5 "},
    "model": {"display_name": "X", "id": "y", "foo": 2}, "session_id": "s",
    "aaa": {"n": [1.0, 2e3, "x", None, True]}, "workspace": None, "vim": {"mode": "NORMAL", "x": 1},
    "rate_limits": {"seven_day": {"resets_at": "5"}, "five_hour": {}}, "context_window": {"current_usage": "12"},
    "terminal_width": "mine", "effort": {"level": None}, "big": 12345678901234567890, "neg": -0.0, "exp": 1e-7,
    "unicode": "é😀\u2028"})
cc("stdin-model-string", "cat", env={"CCSTATUSLINE_WIDTH": "5000"}, stdin={"model": "opus", "session_id": "q"})
cc("stdin-not-read-large", "true", stdin=dict(BASE_STDIN, pad="x" * 300000))
cc("pwd", "pwd")
cc("env-home", "echo \"$HOME\" | sed 's|.*/||'")
cc("ansi-stripped", "printf '\\033[31mred\\033[0m plain'")
cc("ansi-preserved", "printf '\\033[31mred\\033[0m plain'", items_extra={"preserveColors": True})
cc("ansi-preserved-color-override", "printf '\\033[32mgreen\\033[0m'", items_extra={"preserveColors": True, "color": "red"})
cc("osc8-stripped", "printf '\\033]8;;https://x.y\\033\\\\link\\033]8;;\\033\\\\'")
cc("osc8-preserved", "printf '\\033]8;;https://x.y\\033\\\\link\\033]8;;\\033\\\\'", items_extra={"preserveColors": True})
cc("max-width", "echo abcdefghijklmnop", items_extra={"maxWidth": 8})
cc("max-width-fits", "echo abcdefgh", items_extra={"maxWidth": 8})
cc("max-width-small", "echo abcdefghij", items_extra={"maxWidth": 2})
cc("max-width-fraction", "echo abcdefghij", items_extra={"maxWidth": 5.5})
cc("max-width-below-one", "echo abcdefghij", items_extra={"maxWidth": 0.5}, more_items=[w("version")])
cc("max-width-zero", "echo abcdefghij", items_extra={"maxWidth": 0})
cc("max-width-negative", "echo abcdefghij", items_extra={"maxWidth": -3})
cc("max-width-colors-reset", "printf '\\033[31mabcdefghijkl'", items_extra={"maxWidth": 6, "preserveColors": True})
cc("max-width-colors-closed", "printf '\\033[31mabcdefghijkl\\033[0m'", items_extra={"maxWidth": 6, "preserveColors": True})
cc("max-width-wide-chars", "echo 日本語テキスト", items_extra={"maxWidth": 7})
cc("long-output", "head -c 20000 /dev/zero | tr '\\0' x; echo", items_extra={"maxWidth": 10})
cc("over-16k-chars", "printf '%016383d' 0; printf '😀tail'")
cc("over-16k-chars-max-width", "printf '%016383d' 0; printf '😀tail'", items_extra={"maxWidth": 20})
cc("overflow-1mib", "head -c 1048577 /dev/zero | tr '\\0' y")
cc("exactly-1mib", "head -c 1048576 /dev/zero | tr '\\0' y", items_extra={"maxWidth": 4})
cc("invalid-utf8", "printf 'a\\377b\\300\\200c'")
cc("empty-command", "", more_items=[w("version")])
cc("no-command", None, more_items=[w("version")])
cc("rendered-twice", "echo same", more_items=[w(CC, commandPath="echo same")])
COUNTER = "n=$(cat count 2>/dev/null || echo 0); n=$((n+1)); echo $n > count; echo run$n"
cc("counter-no-ttl", COUNTER, more_items=[w(CC, commandPath=COUNTER)])
cc("counter-ttl-memory", COUNTER, more_items=[w(CC, commandPath=COUNTER)],
   config_extra={"customCommandCacheTtlSeconds": 10}, homeTreeIgnore=[])
cc("counter-ttl-no-session", COUNTER, more_items=[w(CC, commandPath=COUNTER)],
   config_extra={"customCommandCacheTtlSeconds": 10}, drop=["session_id"], homeTreeIgnore=[])
cc("counter-ttl-different-timeout", COUNTER, more_items=[w(CC, commandPath=COUNTER, timeout=999)],
   config_extra={"customCommandCacheTtlSeconds": 10}, homeTreeIgnore=[])
cc("ttl-failed-cached", "exit 3", more_items=[w(CC, commandPath="exit 3")],
   config_extra={"customCommandCacheTtlSeconds": 60}, homeTreeIgnore=[], env={"CCSTATUSLINE_WIDTH": "100"})
cc("ttl-max", "echo cached", config_extra={"customCommandCacheTtlSeconds": 60}, homeTreeIgnore=[])
cc("ttl-special-chars", "printf 'q\"\\\\\\t'", config_extra={"customCommandCacheTtlSeconds": 5}, homeTreeIgnore=[])

# minimalist mode forces rawValue
for t, extra in [("session-cost", {}), ("current-working-dir", {"character": ">"}), ("link", {"metadata": {"url": "https://e.x/a b"}}),
                 ("sandbox-status", {}), ("remote-control-status", {}), ("free-memory", {}), ("version", {}), ("output-style", {})]:
    case(t, "minimalist", w(t, **extra), config_extra={"minimalistMode": True},
         files={USER: {"json": {"sandbox": {"enabled": True}}},
                SESS + "1.json": {"json": {"sessionId": "parity-session-0001", "bridgeSessionId": "b"}}})

with_dir = OUT
# drop old cases (only *.json; this script may live in the same directory)
for root, dirs, files in os.walk(with_dir):
    for fn in files:
        if fn.endswith(".json"):
            os.remove(os.path.join(root, fn))
for c in cases:
    if c["config"]["lines"][0][0].get("commandPath", "") is None:
        del c["config"]["lines"][0][0]["commandPath"]
    path = os.path.join(with_dir, c["name"].split("/", 1)[1] + ".json")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(c, f, indent=2, ensure_ascii=False)
        f.write("\n")
print(len(cases), "cases")
