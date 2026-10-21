#!/usr/bin/env python3
"""Generates the git widget group's parity cases (parity/corpus/git/**.json).

Run: python3 parity/corpus/git/generate.py   (rewrites every case file)
"""
import json
import os
import shutil

ROOT = os.path.dirname(os.path.abspath(__file__))
CASES = []

BASE_STDIN = {
    "hook_event_name": "Status",
    "session_id": "parity-git-0001",
    "model": {"id": "claude-opus-4-1-20250805", "display_name": "Opus 4.1"},
    "version": "2.0.37",
}

MODEL = {"id": "m", "type": "model"}
SEP = {"id": "s", "type": "separator"}

FIXTURE_DIRS = {
    "no-git": "no-git",
    "git-clean": "git-clean",
    "git-empty": "git-empty",
    "git-dirty": "git-dirty",
    "git-ahead-behind": "git-ahead-behind",
    "git-worktree-main": "git-worktree/main",
    "git-worktree-linked": "git-worktree/linked",
    "git-conflict": "git-conflict",
    "git-detached": "git-detached",
    "git-symlink": "git-symlink/link",
    "git-filters": "git-filters",
    "git-renames": "git-renames",
    "git-conflict-spaces": "git-conflict-spaces",
    "git-tracking": "git-tracking",
}

ALL_FIXTURES = list(FIXTURE_DIRS)


def fixture_name(key):
    return FIXTURE_DIRS[key].split("/")[0]


def add(name, items, where="git-clean", *, tags=(), stdin=None, cwd_override=None, files=None, env=None,
        path_prepend=None, args=None, config_extra=None, home_ignore=None, fixtures=None, process_cwd=None,
        no_stdin_cwd=False, raw_config=None):
    """One case: `items` is a line of widget items (ids are assigned)."""
    line = []
    for i, it in enumerate(items):
        it = dict(it)
        it.setdefault("id", str(i + 1))
        line.append(it)
    config = {"version": 4, "colorLevel": 2, "lines": [line]}
    if config_extra:
        config.update(config_extra)
    case = {"name": f"git/{name}", "tags": sorted(set(["git-group", *tags, *[i["type"] for i in items]]))}
    fx = fixtures if fixtures is not None else ([fixture_name(where)] if where else [])
    if fx:
        case["fixtures"] = fx
    cwd_path = "${CASE_DIR}/" + FIXTURE_DIRS[where] if where else "${CASE_DIR}"
    case["cwd"] = process_cwd if process_cwd is not None else (FIXTURE_DIRS[where] if where else ".")
    case["config"] = raw_config if raw_config is not None else config
    s = dict(BASE_STDIN)
    if not no_stdin_cwd:
        c = cwd_override if cwd_override is not None else cwd_path
        s["cwd"] = c
        s["workspace"] = {"current_dir": c, "project_dir": c}
    if stdin:
        s.update(stdin)
    case["stdin"] = s
    for k, v in (("files", files), ("env", env), ("pathPrepend", path_prepend), ("args", args),
                 ("homeTreeIgnore", home_ignore)):
        if v is not None:
            case[k] = v
    CASES.append(case)


def w(t, **kw):
    return {"type": t, **kw}


# ------------------------------------------------------------------ matrix

GIT_WIDGETS = [
    "git-branch", "git-changes", "git-insertions", "git-deletions", "git-staged-files", "git-unstaged-files",
    "git-untracked-files", "git-clean-status", "git-root-dir", "git-worktree", "git-status", "git-staged",
    "git-unstaged", "git-untracked", "git-ahead-behind", "git-conflicts", "git-sha", "git-origin-owner",
    "git-origin-repo", "git-origin-owner-repo", "git-upstream-owner", "git-upstream-repo",
    "git-upstream-owner-repo", "git-is-fork", "git-review", "git-ci-status",
]

for t in GIT_WIDGETS:
    for fx in ALL_FIXTURES:
        add(f"{t}/{fx}", [w(t)], fx)
    # raw value everywhere it matters
    for fx in ("git-dirty", "git-ahead-behind", "git-conflict", "no-git"):
        add(f"{t}/{fx}-raw", [w(t, rawValue=True)], fx, tags=["rawValue"])
    # every hide state at once, behind a visible widget
    hide = "no-git,no-remote,no-upstream,not-fork,zero,no-data,status,title"
    for fx in ("git-clean", "no-git", "git-empty"):
        add(f"{t}/{fx}-hide-all", [MODEL, SEP, w(t, metadata={"hide": hide})], fx, tags=["hide"])
    add(f"{t}/git-clean-hide-empty-list", [MODEL, SEP, w(t, metadata={"hide": ""})], "git-clean", tags=["hide"])

# all git widgets in one line: the prefetch has to serve all of them
ALL_LINE = []
for t in GIT_WIDGETS:
    ALL_LINE += [w(t), SEP]
for fx in ALL_FIXTURES:
    add(f"all/{fx}", ALL_LINE[:-1], fx)
    add(f"all/{fx}-minimalist", ALL_LINE[:-1], fx, config_extra={"minimalistMode": True}, tags=["minimalist"])
add("all/git-dirty-ttl0", ALL_LINE[:-1], "git-dirty", config_extra={"gitCacheTtlSeconds": 0})
add("all/git-dirty-ttl60", ALL_LINE[:-1], "git-dirty", config_extra={"gitCacheTtlSeconds": 60})
# the hot trio of the live config
HOT = [w("git-branch"), SEP, w("git-root-dir"), SEP, w("git-changes")]
for fx in ALL_FIXTURES:
    add(f"hot/{fx}", HOT, fx)
add("hot/two-lines", HOT, "git-dirty", config_extra={"lines": [HOT, [w("git-status"), SEP, w("git-sha")]]})

# ------------------------------------------------------------------ cwd sources

add("cwd/workspace-current-dir-only", HOT, "git-dirty", no_stdin_cwd=True,
    stdin={"workspace": {"current_dir": "${CASE_DIR}/git-dirty"}})
add("cwd/project-dir-only", HOT, "git-dirty", no_stdin_cwd=True,
    stdin={"workspace": {"project_dir": "${CASE_DIR}/git-dirty"}})
add("cwd/blank-cwd-falls-through", HOT, "git-dirty", no_stdin_cwd=True,
    stdin={"cwd": "   ", "workspace": {"current_dir": "", "project_dir": "${CASE_DIR}/git-dirty"}})
add("cwd/none-uses-process-cwd", HOT, "git-dirty", no_stdin_cwd=True)
add("cwd/none-process-cwd-no-git", HOT, "no-git", no_stdin_cwd=True)
add("cwd/relative", HOT, "git-dirty", cwd_override="git-dirty", process_cwd=".")
add("cwd/relative-dotdot", HOT, "git-dirty", cwd_override="../git-dirty/src", process_cwd="no-git",
    fixtures=["git-dirty", "no-git"])
add("cwd/subdirectory", HOT, "git-dirty", cwd_override="${CASE_DIR}/git-dirty/src")
add("cwd/missing-dir", HOT, "git-dirty", cwd_override="${CASE_DIR}/git-dirty/does/not/exist")
add("cwd/missing-dir-outside", HOT, "no-git", cwd_override="${CASE_DIR}/nowhere/at/all")
add("cwd/is-a-file", HOT, "git-dirty", cwd_override="${CASE_DIR}/git-dirty/README.md")
add("cwd/inside-dot-git", ALL_LINE[:-1], "git-dirty", cwd_override="${CASE_DIR}/git-dirty/.git")
add("cwd/inside-dot-git-objects", HOT, "git-dirty", cwd_override="${CASE_DIR}/git-dirty/.git/objects")
add("cwd/symlink", ALL_LINE[:-1], "git-symlink")
add("cwd/trailing-slash", HOT, "git-dirty", cwd_override="${CASE_DIR}/git-dirty/")
add("cwd/linked-worktree-subdir", HOT + [SEP, w("git-worktree")], "git-worktree-linked")
add("cwd/git-dir-env", HOT, "git-dirty", env={"GIT_DIR": "${CASE_DIR}/git-dirty/.git"},
    cwd_override="${CASE_DIR}/git-dirty/src")
add("cwd/git-dir-env-outside", HOT, "no-git", fixtures=["no-git", "git-dirty"],
    env={"GIT_DIR": "${CASE_DIR}/git-dirty/.git"})

# ------------------------------------------------------------------ git-branch

B = "git-branch"
for fx in ("git-dirty", "git-detached", "git-empty", "no-git"):
    add(f"{B}/opt/{fx}-no-symbol", [w(B, character="")], fx)
    add(f"{B}/opt/{fx}-custom-symbol", [w(B, character="★")], fx)
for mw in (1, 1.5, 3, 4, 5, 8, 100, 0, -2):
    add(f"{B}/opt/max-width-{mw}", [w(B, maxWidth=mw)], "git-worktree-linked")
    add(f"{B}/opt/max-width-{mw}-raw", [w(B, maxWidth=mw, rawValue=True)], "git-detached")
for fx in ("git-ahead-behind", "git-clean", "git-detached", "git-tracking", "git-worktree-linked", "no-git"):
    add(f"{B}/opt/link-{fx}", [w(B, metadata={"linkToRepo": "true"})], fx, tags=["link"])
add(f"{B}/opt/link-legacy-github", [w(B, metadata={"linkToGitHub": "true"})], "git-ahead-behind", tags=["link"])
add(f"{B}/opt/link-false-overrides-legacy", [w(B, metadata={"linkToRepo": "false", "linkToGitHub": "true"})],
    "git-ahead-behind", tags=["link"])
add(f"{B}/opt/link-max-width", [w(B, maxWidth=6, metadata={"linkToRepo": "true"})], "git-ahead-behind",
    tags=["link"])
add(f"{B}/opt/hide-no-git", [MODEL, SEP, w(B, metadata={"hide": "no-git"})], "no-git", tags=["hide"])
HEADS = {
    "unicode-branch": "ref: refs/heads/feature/ünï 😀#?%\n",
    "unicode-valid": "ref: refs/heads/feature/ünï-😀#%\n",
    "question-mark": "ref: refs/heads/what?\n",
    "dot-lock": "ref: refs/heads/x.lock\n",
    "dot-component": "ref: refs/heads/.hidden\n",
    "at-brace": "ref: refs/heads/a@{b}\n",
    "plus-amp": "ref: refs/heads/a+b&c\n",
    "remote-ref": "ref: refs/remotes/origin/main\n",
    "tag-ref": "ref: refs/tags/v1\n",
    "heads-only": "ref: refs/heads/\n",
    "no-space": "ref:refs/heads/main\n",
    "extra-spaces": "ref:   refs/heads/main  \n",
    "no-newline": "ref: refs/heads/main",
    "crlf": "ref: refs/heads/main\r\n",
    "heads-heads": "ref: refs/heads/heads/x\n",
    "long-name": "ref: refs/heads/feature/a-really-long-branch-name-that-goes-on\n",
}
for label, head in HEADS.items():
    for fx in ("git-ahead-behind",):
        files = {"${CASE_DIR}/" + FIXTURE_DIRS[fx] + "/.git/HEAD": head}
        add(f"{B}/head/{label}", [w(B), SEP, w(B, rawValue=True), SEP, w(B, metadata={"linkToRepo": "true"}),
                                  SEP, w("git-sha")], fx, files=files, tags=["link"])
add(f"{B}/head/linked-worktree-head", [w(B)], "git-worktree-linked",
    files={"${CASE_DIR}/git-worktree/main/.git/worktrees/linked/HEAD": "ref: refs/heads/other/branch\n"})
add(f"{B}/head/reftable-marker", [w(B)], "git-clean",
    files={"${CASE_DIR}/git-clean/.git/HEAD": "ref: refs/heads/.invalid\n"})

# ------------------------------------------------------------------ line counts

for t in ("git-changes", "git-insertions", "git-deletions"):
    sym = {"symbolInsertions": "ins:", "symbolDeletions": "del:"}
    for fx in ("git-dirty", "git-filters", "git-renames", "git-conflict"):
        add(f"{t}/opt/{fx}-symbols", [w(t, metadata=sym)], fx)
        add(f"{t}/opt/{fx}-empty-symbols", [w(t, metadata={"symbolInsertions": "", "symbolDeletions": ""})], fx)
    add(f"{t}/opt/hide-zero-clean", [MODEL, SEP, w(t, metadata={"hide": "zero"})], "git-clean", tags=["hide"])
    add(f"{t}/opt/hide-zero-dirty", [MODEL, SEP, w(t, metadata={"hide": "zero"})], "git-dirty", tags=["hide"])
    add(f"{t}/opt/hide-zero-spaces", [MODEL, SEP, w(t, metadata={"hide": " zero , no-git "})], "git-clean",
        tags=["hide"])

# ------------------------------------------------------------------ file counts

for t in ("git-staged-files", "git-unstaged-files", "git-untracked-files"):
    for fx in ("git-dirty", "git-renames", "git-conflict", "git-filters"):
        add(f"{t}/opt/{fx}-label", [w(t, metadata={"label": "Files: "})], fx)
        add(f"{t}/opt/{fx}-empty-label", [w(t, metadata={"label": ""})], fx)
        add(f"{t}/opt/{fx}-label-raw", [w(t, rawValue=True, metadata={"label": "X"})], fx)
    add(f"{t}/opt/hide-zero", [MODEL, SEP, w(t, metadata={"hide": "zero"})], "git-clean", tags=["hide"])

# ------------------------------------------------------------------ status family

for fx in ("git-dirty", "git-conflict", "git-renames", "git-clean", "git-empty"):
    add(f"git-clean-status/opt/{fx}-symbols", [w("git-clean-status", metadata={"symbolClean": "OK",
                                                                          "symbolDirty": "XX"})], fx)
    add(f"git-status/opt/{fx}-symbols", [w("git-status", metadata={
        "symbolConflicts": "C", "symbolStaged": "S", "symbolUnstaged": "U", "symbolUntracked": "N"})], fx)
    add(f"git-status/opt/{fx}-empty-symbols", [MODEL, SEP, w("git-status", metadata={
        "symbolConflicts": "", "symbolStaged": "", "symbolUnstaged": "", "symbolUntracked": ""})], fx)
    for t in ("git-staged", "git-unstaged", "git-untracked"):
        add(f"{t}/opt/{fx}-character", [w(t, character="◉")], fx)
        add(f"{t}/opt/{fx}-character-raw", [w(t, character="◉", rawValue=True)], fx)

# ------------------------------------------------------------------ root dir

R = "git-root-dir"
for mode in ("vscode", "cursor", "emacs", ""):
    for fx in ("git-dirty", "git-worktree-linked", "git-symlink"):
        add(f"{R}/opt/link-{mode or 'empty'}-{fx}", [w(R, metadata={"linkToIDE": mode})], fx, tags=["link"])
add(f"{R}/opt/link-legacy-cursor", [w(R, metadata={"linkToCursor": "true"})], "git-dirty", tags=["link"])
add(f"{R}/opt/link-ide-beats-legacy", [w(R, metadata={"linkToIDE": "vscode", "linkToCursor": "true"})],
    "git-dirty", tags=["link"])
for mw in (2, 3, 4, 6, 50):
    add(f"{R}/opt/max-width-{mw}", [w(R, maxWidth=mw)], "git-dirty")
    add(f"{R}/opt/max-width-{mw}-link", [w(R, maxWidth=mw, metadata={"linkToIDE": "vscode"})], "git-dirty",
        tags=["link"])

# ------------------------------------------------------------------ worktree

for fx in ("git-worktree-main", "git-worktree-linked", "git-dirty", "no-git"):
    add(f"git-worktree/opt/{fx}-character", [w("git-worktree", character="WT")], fx)
    add(f"git-worktree/opt/{fx}-no-character", [w("git-worktree", character="")], fx)

# ------------------------------------------------------------------ ahead/behind

A = "git-ahead-behind"
ab_sym = {"symbolAhead": "+", "symbolBehind": "-"}
for fx in ("git-ahead-behind", "git-tracking", "git-clean", "git-detached"):
    add(f"{A}/opt/{fx}-symbols", [w(A, metadata=ab_sym)], fx)
    add(f"{A}/opt/{fx}-show-zero", [MODEL, SEP, w(A, metadata={"hide": "no-git"})], fx, tags=["hide"])
    add(f"{A}/opt/{fx}-show-zero-raw", [MODEL, SEP, w(A, rawValue=True, metadata={"hide": "no-git"})], fx,
        tags=["hide"])
    add(f"{A}/opt/{fx}-hide-no-upstream", [MODEL, SEP, w(A, metadata={"hide": "no-upstream"})], fx,
        tags=["hide"])
add(f"{A}/opt/behind-only", [w(A)], "git-ahead-behind",
    files={"${CASE_DIR}/git-ahead-behind/.git/HEAD": "ref: refs/remotes/origin/main\n"})

# ------------------------------------------------------------------ conflicts

C = "git-conflicts"
for fx in ("git-conflict", "git-conflict-spaces", "git-clean", "git-dirty"):
    add(f"{C}/opt/{fx}-zero-clean", [w(C, metadata={"zeroDisplay": "clean"})], fx)
    add(f"{C}/opt/{fx}-zero-clean-raw", [w(C, rawValue=True, metadata={"zeroDisplay": "clean"})], fx)
    add(f"{C}/opt/{fx}-zero-clean-symbol", [w(C, metadata={"zeroDisplay": "clean", "symbolClean": "ok"})], fx)
    add(f"{C}/opt/{fx}-zero-bogus", [w(C, metadata={"zeroDisplay": "bogus"})], fx)
    add(f"{C}/opt/{fx}-character", [w(C, character="!")], fx)
    add(f"{C}/opt/{fx}-hide-zero", [MODEL, SEP, w(C, metadata={"hide": "zero", "zeroDisplay": "clean"})], fx,
        tags=["hide"])

# ------------------------------------------------------------------ sha

for fx in ("git-empty", "git-detached", "no-git"):
    add(f"git-sha/opt/{fx}-hide-no-git", [MODEL, SEP, w("git-sha", metadata={"hide": "no-git"})], fx,
        tags=["hide"])

# ------------------------------------------------------------------ remotes

REMOTE_WIDGETS = ["git-origin-owner", "git-origin-repo", "git-origin-owner-repo", "git-upstream-owner",
                  "git-upstream-repo", "git-upstream-owner-repo", "git-is-fork"]
REMOTE_LINE = []
for t in REMOTE_WIDGETS:
    REMOTE_LINE += [w(t), SEP, w(t, metadata={"linkToRepo": "true"}), SEP]
REMOTE_LINE += [w("git-origin-owner-repo", metadata={"ownerOnlyWhenFork": "true"}), SEP,
                w("git-branch", metadata={"linkToRepo": "true"}), SEP, w("git-review")]

URLS = {
    "scp": "git@github.com:acme/widgets.git",
    "scp-no-user": "github.com:acme/widgets",
    "scp-trailing-slash": "git@github.com:acme/widgets.git/",
    "scp-subgroups": "git@gitlab.com:group/sub/proj.git",
    "scp-user-with-at": "a@b@host.example:o/r",
    "scp-only-repo": "git@github.com:widgets.git",
    "scp-double-git": "git@github.com:acme/widgets.git.git",
    "https": "https://github.com/acme/widgets.git",
    "https-upper-host": "https://GitHub.COM/Acme/Widgets.git",
    "https-port-default": "https://github.com:443/acme/widgets.git",
    "https-port": "https://git.example.com:8443/acme/widgets.git",
    "https-userinfo": "https://user:pass@github.com/acme/widgets.git",
    "https-slashes": "https://github.com//acme//widgets.git//",
    "https-dotdot": "https://github.com/acme/x/../widgets",
    "https-percent": "https://github.com/ac%20me/wid%2Fgets.git",
    "https-space": "https://github.com/ac me/widgets",
    "https-query": "https://github.com/acme/widgets.git?x=1#frag",
    "https-backslash": "https://github.com\\acme\\widgets",
    "https-ipv4-hex": "https://0x7f.1/acme/widgets",
    "https-ipv6": "https://[::1]:8080/acme/widgets",
    "https-idn": "https://bücher.example/acme/widgets",
    "https-no-owner": "https://github.com/widgets",
    "http": "http://gitea.local:80/acme/widgets",
    "ssh": "ssh://git@github.com/acme/widgets.git",
    "ssh-port": "ssh://git@Host.Example:2222/acme/widgets.git",
    "ssh-upper": "SSH://git@github.com/acme/widgets",
    "git-protocol": "git://github.com/acme/widgets",
    "file": "file:///srv/git/acme/widgets.git",
    "local-path": "/srv/git/acme/widgets.git",
    "relative-path": "../widgets",
    "https-single-slash": "https:/github.com/acme/widgets",
    "gitlab-https": "https://gitlab.com/group/sub/proj.git",
    "whitespace": "  https://github.com/acme/widgets.git  ",
}


def git_config(remotes, extra=""):
    s = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n"
    for name, url in remotes:
        q = url.replace("\\", "\\\\").replace('"', '\\"')
        s += f'[remote "{name}"]\n\turl = "{q}"\n\tfetch = +refs/heads/*:refs/remotes/{name}/*\n'
    return s + extra


for label, url in URLS.items():
    files = {"${CASE_DIR}/git-clean/.git/config": git_config([("origin", url)])}
    add(f"remote/origin-{label}", REMOTE_LINE, "git-clean", files=files, tags=["link"])
    files = {"${CASE_DIR}/git-clean/.git/config": git_config(
        [("origin", "https://github.com/acme/widgets.git"), ("upstream", url)])}
    add(f"remote/upstream-{label}", REMOTE_LINE, "git-clean", files=files, tags=["link"])
add("remote/no-remotes", REMOTE_LINE, "git-clean", tags=["link"])
add("remote/no-git", REMOTE_LINE, "no-git", tags=["link"])
add("remote/ahead-behind", REMOTE_LINE, "git-ahead-behind", tags=["link"])
add("remote/tracking", REMOTE_LINE, "git-tracking", tags=["link"])
add("remote/same-owner-repo-not-fork", REMOTE_LINE, "git-clean", tags=["link"], files={
    "${CASE_DIR}/git-clean/.git/config": git_config([("origin", "git@github.com:acme/widgets.git"),
                                                     ("upstream", "https://github.com/acme/widgets")])})
add("remote/fork-remote-only", REMOTE_LINE, "git-clean", tags=["link"], files={
    "${CASE_DIR}/git-clean/.git/config": git_config([("origin", "git@github.com:acme/widgets.git"),
                                                     ("fork", "https://github.com/me/widgets")])})
add("remote/upstream-unparseable-falls-to-fork", REMOTE_LINE, "git-clean", tags=["link"], files={
    "${CASE_DIR}/git-clean/.git/config": git_config([("origin", "git@github.com:acme/widgets.git"),
                                                     ("upstream", "/local/path"),
                                                     ("fork", "https://github.com/me/widgets")])})
for t in REMOTE_WIDGETS:
    for fx in ("git-clean", "no-git"):
        add(f"{t}/opt/{fx}-hide", [MODEL, SEP, w(t, metadata={"hide": "no-remote,no-upstream,not-fork"})], fx,
            tags=["hide"])
for fx in ("git-ahead-behind", "git-clean"):
    add(f"git-is-fork/opt/{fx}-character", [w("git-is-fork", character="F")], fx)
    add(f"git-is-fork/opt/{fx}-character-raw", [w("git-is-fork", character="F", rawValue=True)], fx)

# ------------------------------------------------------------------ git-review / ci (cache file)

REVIEW_LINE = [w("git-review"), SEP, w("git-review", rawValue=True), SEP,
               w("git-review", metadata={"hide": "status"}), SEP, w("git-review", metadata={"hide": "title"}), SEP,
               w("git-pr"), SEP, w("git-ci-status"), SEP, w("git-ci-status", rawValue=True)]


def pr(**kw):
    d = {"number": 42, "url": "https://github.com/acme/widgets/pull/42", "title": "Add the thing",
         "state": "OPEN", "reviewDecision": "", "provider": "gh"}
    d.update(kw)
    return {k: v for k, v in d.items() if v is not None}


def stored(data, cq=True):
    return json.dumps({"version": 1, "data": data, "checksQueried": cq})


CHECKS = {"state": "failing", "failing": 2, "pending": 1, "success": 7}
REVIEW_CACHES = {
    "missing": None,
    "legacy-empty": "",
    "legacy-whitespace": "  \n",
    "no-pr": stored(None),
    "open": stored(pr()),
    "open-checks": stored(pr(checks=CHECKS)),
    "approved": stored(pr(reviewDecision="APPROVED")),
    "changes-requested": stored(pr(reviewDecision="CHANGES_REQUESTED")),
    "merged-approved": stored(pr(state="MERGED", reviewDecision="APPROVED")),
    "closed": stored(pr(state="CLOSED")),
    "draft-state": stored(pr(state="DRAFT")),
    "empty-state": stored(pr(state="")),
    "empty-title": stored(pr(title="")),
    "long-title": stored(pr(title="A very long pull request title that keeps going and going")),
    "exact-30-title": stored(pr(title="x" * 30)),
    "31-title": stored(pr(title="y" * 31)),
    "cjk-title": stored(pr(title="修复一个非常重要的问题并且添加测试用例和文档")),
    "emoji-title": stored(pr(title="🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀 launch")),
    "glab": stored(pr(provider="glab", url="https://gitlab.com/g/p/-/merge_requests/7", number=7)),
    "no-provider-gitlab-url": stored(pr(provider=None, url="https://gitlab.example/g/p/-/merge_requests/7")),
    "no-provider-github-url": stored(pr(provider=None)),
    "fractional-number": stored(pr(number=42.5)),
    "big-number": stored(pr(number=12345678901234567890)),
    "url-control-char": stored(pr(url="https://github.com/acme/widgets/pull/42\u0007")),
    "checks-passing": stored(pr(checks={"state": "passing", "failing": 0, "pending": 0, "success": 3})),
    "checks-zero": stored(pr(checks={"state": "passing", "failing": 0, "pending": 0, "success": 0})),
    "checks-pending-zero": stored(pr(checks={"state": "pending", "failing": 0, "pending": 0, "success": 0})),
    "checks-weird-state": stored(pr(checks={"state": "weird", "failing": 0, "pending": 0, "success": 0})),
    "checks-string-counts": stored(pr(checks={"state": "failing", "failing": "3", "pending": "0", "success": True})),
    "checks-null": stored({**pr(), "checks": None}),
    "checks-not-queried": stored(pr(), cq=False),
    "legacy-data": json.dumps(pr()),
    "legacy-data-checks": json.dumps(pr(checks=CHECKS)),
    "bad-version": json.dumps({"version": 2, "data": pr(), "checksQueried": True}),
    "bad-json": "{nope",
    "json-null": "null",
    "json-array": "[1,2]",
    "number-string": stored(pr(number="42")) ,
    "extra-fields": stored({**pr(), "extra": {"a": 1}}),
    "title-number": stored(pr(title=5)),
    "state-number": stored(pr(state=7)),
    "state-array": stored(pr(state=["A", "B"])),
    "review-decision-only": stored(pr(state="WEIRD", reviewDecision="APPROVED")),
}
for label, content in REVIEW_CACHES.items():
    files = {} if content is None else {"${CASE_DIR}/git-review.review.json": content}
    add(f"git-review/cache/{label}", REVIEW_LINE, None, fixtures=["git-review"], process_cwd="git-review",
        cwd_override="${CASE_DIR}/git-review", files=files)
# the only line: hide no-data
for label in ("missing", "no-pr", "open"):
    content = REVIEW_CACHES[label]
    files = {} if content is None else {"${CASE_DIR}/git-review.review.json": content}
    add(f"git-review/cache/{label}-hide-no-data", [MODEL, SEP, w("git-review", metadata={"hide": "no-data"}), SEP,
                                                   w("git-ci-status", metadata={"hide": "no-data"})],
        None, fixtures=["git-review"], process_cwd="git-review", cwd_override="${CASE_DIR}/git-review",
        files=files, tags=["hide"])
# corrupted entries upstream throws on (missing title / state with the segment shown)
for label, data in (("title-missing", pr(title=None)), ("state-missing", pr(state=None)),
                    ("title-null", {**pr(), "title": None})):
    add(f"git-review/cache/{label}", [w("git-review")], None, fixtures=["git-review"],
        process_cwd="git-review", cwd_override="${CASE_DIR}/git-review",
        files={"${CASE_DIR}/git-review.review.json": stored(data)})
    add(f"git-review/cache/{label}-segment-hidden", [w("git-review", metadata={"hide": "title,status"})], None,
        fixtures=["git-review"], process_cwd="git-review", cwd_override="${CASE_DIR}/git-review",
        files={"${CASE_DIR}/git-review.review.json": stored(data)})
# no data: the noun comes from origin's host
add("git-review/noun/gitlab-origin", REVIEW_LINE, "git-clean", files={
    "${CASE_DIR}/git-clean/.git/config": git_config([("origin", "git@gitlab.example.com:g/p.git")])})
add("git-review/noun/gitlab-origin-no-git", [w("git-review")], "no-git")
add("git-review/noun/process-cwd", REVIEW_LINE, None, fixtures=["git-review"], process_cwd="git-review",
    no_stdin_cwd=True, files={"${CASE_DIR}/git-review.review.json": stored(pr())})

# ------------------------------------------------------------------ git-review refresh mode (fake gh)

GH_LOG = 'echo "gh $*" >> "$HOME/gh.log"\n'


def fake_gh(body):
    return {"executable": "#!/bin/sh\n" + GH_LOG + body}


PR_JSON = json.dumps({"number": 77, "url": "https://github.com/acme/widgets/pull/77", "title": "Login form",
                      "state": "OPEN", "reviewDecision": "REVIEW_REQUIRED",
                      "statusCheckRollup": [
                          {"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "SUCCESS"},
                          {"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "FAILURE"},
                          {"__typename": "CheckRun", "status": "IN_PROGRESS", "conclusion": ""},
                          {"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "SKIPPED"},
                          {"__typename": "StatusContext", "state": "PENDING"},
                          {"__typename": "StatusContext", "state": "success"}]})
GH_SCRIPTS = {
    "ok": f"case \"$1\" in --version) echo 'gh version 2.0';; pr) printf '%s\\n' '{PR_JSON}';; esac\n",
    "no-pr": "case \"$1\" in --version) echo 'gh version 2.0';; pr) echo 'no pull requests found' >&2; exit 1;; esac\n",
    "checks-unavailable": "case \"$1\" in --version) echo ok;; pr) case \"$*\" in *statusCheckRollup*) "
                          "echo 'Resource not accessible by integration' >&2; exit 1;; *) printf '%s\\n' "
                          "'{\"number\":5,\"url\":\"https://github.com/acme/widgets/pull/5\",\"title\":\"T\"}';; "
                          "esac;; esac\n",
    "empty-output": "case \"$1\" in --version) echo ok;; pr) ;; esac\n",
    "bad-json": "case \"$1\" in --version) echo ok;; pr) echo '{nope';; esac\n",
    "not-object": "case \"$1\" in --version) echo ok;; pr) echo '[1]';; esac\n",
    "missing-number": "case \"$1\" in --version) echo ok;; pr) echo '{\"url\":\"u\"}';; esac\n",
    "no-cli": "exit 127\n",
    "pinned-only": "case \"$1\" in --version) echo ok;; pr) case \"$*\" in *--repo*) printf '%s\\n' "
                   "'{\"number\":9,\"url\":\"https://github.com/acme/widgets/pull/9\",\"title\":\"Pinned\","
                   "\"state\":\"MERGED\"}';; *) exit 1;; esac;; esac\n",
}
for label, body in GH_SCRIPTS.items():
    for mode in ("checks", "metadata"):
        add(f"refresh/{label}-{mode}", [MODEL], None, fixtures=["git-review-repo"], process_cwd="git-review-repo",
            cwd_override="${CASE_DIR}/git-review-repo", path_prepend=["fakebin"],
            files={"${CASE_DIR}/fakebin/gh": fake_gh(body)},
            args=["--internal-refresh-git-review-cache", "${CASE_DIR}/git-review-repo", mode,
                  "${HOME}/.cache/ccstatusline/git-review/not-the-lock.lock"],
            home_ignore=[".cache/ccstatusline/git-cache/"], tags=["refresh"])
for label, a in (("bad-mode", ["${CASE_DIR}/git-review-repo", "everything", "/x.lock"]),
                 ("missing-lock", ["${CASE_DIR}/git-review-repo", "checks"]),
                 ("empty-cwd", ["", "checks", "/x.lock"]),
                 ("no-args", [])):
    add(f"refresh/{label}", [MODEL], None, fixtures=["git-review-repo"], process_cwd="git-review-repo",
        cwd_override="${CASE_DIR}/git-review-repo", path_prepend=["fakebin"],
        files={"${CASE_DIR}/fakebin/gh": fake_gh(GH_SCRIPTS["ok"])},
        args=["--internal-refresh-git-review-cache", *a], home_ignore=[".cache/ccstatusline/git-cache/"],
        tags=["refresh"])
# a fresh cache file already there: no gh call at all
add("refresh/fresh-cache-skips-gh", [MODEL], None, fixtures=["git-review"], process_cwd="git-review",
    cwd_override="${CASE_DIR}/git-review", path_prepend=["fakebin"],
    files={"${CASE_DIR}/fakebin/gh": fake_gh(GH_SCRIPTS["ok"]),
           "${CASE_DIR}/git-review.review.json": stored(pr(checks=CHECKS))},
    args=["--internal-refresh-git-review-cache", "${CASE_DIR}/git-review", "checks", "/x.lock"],
    home_ignore=[".cache/ccstatusline/git-cache/"], tags=["refresh"])
add("refresh/gitlab-origin-no-glab", [MODEL], None, fixtures=["git-review-repo"], process_cwd="git-review-repo",
    cwd_override="${CASE_DIR}/git-review-repo", path_prepend=["fakebin"],
    files={"${CASE_DIR}/fakebin/gh": fake_gh(GH_SCRIPTS["ok"]),
           "${CASE_DIR}/fakebin/glab": {"executable": "#!/bin/sh\necho \"glab $*\" >> \"$HOME/glab.log\"\n"
                                        "case \"$1\" in --version) echo ok;; mr) echo '{\"iid\":3,\"web_url\":"
                                        "\"https://gitlab.com/g/p/-/merge_requests/3\",\"title\":\"MR\",\"state\":"
                                        "\"merged\"}';; esac\n"},
           "${CASE_DIR}/git-review-repo/.git/config": git_config([("origin", "https://gitlab.com/g/p.git")])},
    args=["--internal-refresh-git-review-cache", "${CASE_DIR}/git-review-repo", "checks", "/x.lock"],
    home_ignore=[".cache/ccstatusline/git-cache/"], tags=["refresh"])

# ------------------------------------------------------------------ jj

JJ = ["jj-bookmarks", "jj-workspace", "jj-root-dir", "jj-changes", "jj-insertions", "jj-deletions",
      "jj-description", "jj-revision"]
JJ_LINE = []
for t in JJ:
    JJ_LINE += [w(t), SEP, w(t, rawValue=True), SEP]
JJ_LINE += [w("jj-bookmarks", character=""), SEP, w("jj-revision", character="@"), SEP,
            w("jj-changes", metadata={"symbolInsertions": "i", "symbolDeletions": "d"}), SEP,
            w("jj-bookmarks", metadata={"label": "BM "})]
for t in JJ:
    for fx in ("no-git", "git-dirty"):
        add(f"{t}/{fx}", [w(t)], fx)
        add(f"{t}/{fx}-hide", [MODEL, SEP, w(t, metadata={"hide": "no-jj"})], fx, tags=["hide"])
        add(f"{t}/{fx}-character", [w(t, character="J")], fx)

JJ_SCRIPTS = {
    "full": """case "$1" in
root) echo "/home/me/projects/my-jj-repo/";;
log) case "$*" in
  *bookmarks*) printf 'main feature/x\\nrelease\\n';;
  *description*) printf 'Fix the thing\\n';;
  *change_id*) printf 'kkmpptxz\\n';;
  esac;;
workspace) printf '\\n   \\n  default  \\nother\\n';;
diff) printf 'a.txt | 3 ++-\\nb.txt | 1 +\\n2 files changed, 3 insertions(+), 1 deletion(-)\\n';;
esac
""",
    "empty": """case "$1" in
root) echo "/r";;
log) case "$*" in *description*) printf '\\n';; *) ;; esac;;
workspace) ;;
diff) ;;
esac
""",
    "only-root": """case "$1" in root) echo "/r/";; *) exit 1;; esac
""",
    "root-fails": "exit 1\n",
    "odd-stat": """case "$1" in
root) echo "C:\\\\repos\\\\win";;
log) case "$*" in *bookmarks*) printf '  \\t  \\n';; *description*) printf 'multi\\nline\\n';; *) printf '  xy  \\n';; esac;;
workspace) printf 'w1\\r\\nw2\\r\\n';;
diff) printf '1 file changed, 12 insertions(+)\\nsummary 5 insertion, 6 deletions\\n';;
esac
""",
}
for label, body in JJ_SCRIPTS.items():
    for fx in ("no-git", "git-dirty"):
        files = {"${CASE_DIR}/fakebin/jj": {"executable": "#!/bin/sh\n" + body},
                 "${CASE_DIR}/" + FIXTURE_DIRS[fx] + "/.jj/repo/store/type": "git"}
        add(f"jj/fake-{label}-{fx}", JJ_LINE, fx, path_prepend=["fakebin"], files=files)
# a fake jj that would succeed, but no .jj anywhere: jj itself would fail there
add("jj/fake-without-workspace-dir", [w("jj-root-dir")], "no-git", path_prepend=["fakebin"],
    files={"${CASE_DIR}/fakebin/jj": {"executable": "#!/bin/sh\nexit 1\n"}})

# ------------------------------------------------------------------ worktree widgets (status JSON)

WT_LINE = [w("worktree-mode"), SEP, w("worktree-mode", rawValue=True), SEP, w("worktree-mode", character=""), SEP,
           w("worktree-mode", character="WT"), SEP, w("worktree-name"), SEP, w("worktree-branch"), SEP,
           w("worktree-original-branch")]
WT_STDIN = {
    "full": {"worktree": {"name": "feature-x", "path": "/p", "branch": "wt/feature-x", "original_cwd": "/o",
                          "original_branch": "main"}},
    "empty-object": {"worktree": {}},
    "null": {"worktree": None},
    "absent": {},
    "empty-strings": {"worktree": {"name": "", "branch": "", "original_branch": ""}},
}
for label, extra in WT_STDIN.items():
    add(f"worktree-widgets/{label}", WT_LINE, "no-git", stdin=extra)
    add(f"worktree-widgets/{label}-raw-mode-only", [MODEL, SEP, w("worktree-mode", rawValue=True)], "no-git",
        stdin=extra)
    add(f"worktree-widgets/{label}-minimalist", WT_LINE, "no-git", stdin=extra,
        config_extra={"minimalistMode": True})

# ------------------------------------------------------------------ legacy alias

add("git-pr/legacy-alias", [w("git-pr"), SEP, w("git-pr", rawValue=True)], None, fixtures=["git-review"],
    process_cwd="git-review", cwd_override="${CASE_DIR}/git-review",
    files={"${CASE_DIR}/git-review.review.json": stored(pr(checks=CHECKS))})

# ------------------------------------------------------------------ write

for d in os.listdir(ROOT):
    p = os.path.join(ROOT, d)
    if os.path.isdir(p):
        shutil.rmtree(p)
seen = set()
for c in CASES:
    assert c["name"] not in seen, c["name"]
    seen.add(c["name"])
    rel = c["name"][len("git/"):] + ".json"
    path = os.path.join(ROOT, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(c, f, indent=2, ensure_ascii=False)
        f.write("\n")
print(f"{len(CASES)} cases")
