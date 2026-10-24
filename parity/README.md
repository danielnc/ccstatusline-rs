# Parity harness

Differential tests: every corpus case runs through pinned upstream ccstatusline
(`upstream/`, npm `ccstatusline@2.2.32`, run with `node --import preload.mjs`) and
through the Rust binary. The harness compares stdout bytes and exit codes, every file
written under HOME, and optionally stderr and the config file.

```sh
npm install --prefix parity/upstream            # once
cargo build --release                           # the Rust side
node parity/run.mjs                             # all cases vs target/release/ccstatusline-rs
node parity/run.mjs --tag powerline --show-diff # one area, with diffs
node parity/run.mjs --filter widgets/model --upstream-only   # just look at upstream output
node parity/run.mjs --determinism -q            # upstream vs upstream (corpus soundness)
parity/self-check.sh                            # parity run with upstream as the "Rust" binary
node parity/gen/seed.mjs                        # regenerate parity/corpus/ (committed)

# From another worktree: your binary, your group's cases, this tree's harness
node /path/to/main/parity/run.mjs --rust-bin <wt>/target/release/ccstatusline-rs \
    --corpus <wt>/parity/corpus/<group> --show-diff
```

Options: `--rust-bin PATH`, `--filter SUBSTR` and `--tag TAG` (both repeatable, OR),
`--show-diff`, `--json PATH`, `--upstream-only`, `--determinism`, `--corpus DIR`
(repeatable; when given, only those dirs are loaded, not `parity/corpus`),
`--tmp-root DIR`, `--keep-tmp`, `--timeout MS` (default 10000), `--fail-fast`, `-q`.
The report always goes to `parity/last-report.json` (gitignored). Cases run in a
new `<DIR>/ccsl-parity-XXXXXX` (`DIR` defaults to `os.tmpdir()`), and cleanup removes only
that directory, never `DIR` or anything else in it, earlier `--keep-tmp` runs included (`sh parity/tmp-root-check.sh` checks this);
`--keep-tmp` keeps it and prints its path.

**One run at a time, machine-wide.** Cases run sequentially, one process at a time,
and every invocation first takes the lock `<os.tmpdir()>/ccsl-parity-run.lock`
(a directory; `CCSL_PARITY_LOCK_DIR` overrides). A second invocation prints
`waiting for parity lock (held by pid …)` once and retries every 500 ms. A lock whose
owner pid is gone, or that has not been refreshed for 10 minutes (the holder touches
it before every case), is taken over. The lock and the scratch dir are released on
exit, including Ctrl-C / SIGTERM.

**Several corpora.** Case names default to the file path relative to the nearest
directory named `corpus`, so `<wt>/parity/corpus/<group>/x.json` is `<group>/x`. The
same case reached through two `--corpus` dirs runs once; a case with the same name
but different content is an error. A `fixtures/` dir next to that `corpus/` dir is
searched before `parity/fixtures/`.

## World the binary sees

Each side of each case gets a freshly built case dir (`<tmp>/ccsl-parity-XXXXXX/<case>/`,
`<tmp>` = the real path of the scratch parent: `--tmp-root DIR`, else `os.tmpdir()`),
rebuilt from scratch between the two sides so nothing either side writes leaks into the other. Both sides see the same paths.

- `HOME=<case dir>/home`, containing empty `.config/ccstatusline/` and `.claude/`.
- `PATH` = the case's `pathPrepend` dirs, then the harness bin dir, then
  `/usr/bin:/bin:/usr/sbin:/sbin`. The harness bin dir has `git` (whichever the
  parent's PATH finds) and `node`, plus stubs that print an error and exit 127 for
  `gh`, `glab`, `curl`, `wget` and `security`, which shadow any copy in the system
  dirs. Nothing else from the parent PATH is visible (no jj unless a case fakes it).
- The rest of the environment is built from scratch: `HOME`, `TZ=UTC`,
  `LANG=LC_ALL=en_US.UTF-8`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`,
  and the test switches:

| Variable | Default | Meaning |
|---|---|---|
| `CCSL_TEST_NOW_MS` | `1792584000000` (2026-10-21T12:00:00Z) | "now" for every clock read (`Date.now()`, `new Date()`). File mtimes stay real. |
| `CCSL_TEST_NO_PROBE` | `1` | No terminal-width probe (`ps`/`stty`, Linux `/proc/*/fd`): width is unknown unless `CCSTATUSLINE_WIDTH` is set. |
| `CCSL_TEST_NO_NETWORK` | `1` | Every HTTP request fails like an offline connection (ECONNREFUSED). |
| `CCSL_TEST_NO_KEYCHAIN` | `1` | The macOS keychain (`security`) is unavailable; credentials come only from `.credentials.json`. |
| `CCSL_TEST_NO_BACKGROUND` | `1` | Detached background refreshes (git-review/CI cache) are not spawned. |
| `CCSL_TEST_MEM_TOTAL_BYTES` | `17179869184` (16 GiB) | Total memory; also disables `vm_stat`, so used = total - free. |
| `CCSL_TEST_MEM_FREE_BYTES` | `5683281920` (5.3 GiB) | Free memory. |
| `CCSL_TEST_FAKE_BIN_DIRS` | the case's `pathPrepend` dirs | Only set when a case has `pathPrepend`; tells the preload which `gh`/`glab` are fakes. |

`preload.mjs` implements these for upstream. It also always blocks `security`,
`curl`, `wget`, `fetch` and `http(s).request/get`, and blocks `gh`/`glab` unless they
resolve into a `pathPrepend` dir: no real forge CLI call ever happens (every agent
shares one GitHub token). git, jj and sh are never blocked. The Rust binary must
honour the same variables natively.

## Case format

One JSON file per case under `corpus/` (a file may also hold an array, or
`{"cases": [...]}`). Name defaults to the path without `.json` (see above).

```jsonc
{
  "name": "widgets/git-branch/git-dirty",
  "tags": ["git-branch", "git"],          // widget types + features; summary counts per tag
  "fixtures": ["git-dirty"],              // run fixtures/<name>.sh <case dir>/<name>
  "cwd": "git-dirty",                     // process cwd, relative to the case dir (default: case dir)
  "config": { "version": 4, "lines": [[{ "id": "1", "type": "git-branch" }]] },
  // or "configRaw": "{ not json"         // written verbatim
  // "useDefaultConfigPath": true         // write to HOME/.config/ccstatusline/settings.json, no --config
  // "noConfig": true                     // write nothing, pass no --config (default path, missing)
  // neither config nor configRaw         // --config <case dir>/config/settings.json, file missing
  "stdin": { "cwd": "${CASE_DIR}/git-dirty" },   // JSON.stringify'd; or "stdinRaw": "..." verbatim
  "args": [],                             // appended after --config <path>
  "env": { "CCSTATUSLINE_WIDTH": "80", "TZ": null },  // null deletes a variable
  "nowMs": 1792584000000,                 // shorthand for CCSL_TEST_NOW_MS
  "pathPrepend": ["fakebin"],             // dirs (relative to the case dir, or ${CASE_DIR}/…)
                                          // put first on PATH; created if missing
  "files": {                              // relative paths are under HOME; absolute ones as-is
    ".claude.json": { "json": { "oauthAccount": { "emailAddress": "a@b.c" } } },
    "${CASE_DIR}/no-git/.claude/settings.local.json": "{\"sandbox\":{\"enabled\":true}}",
    "${CASE_DIR}/fakebin/gh": { "executable": "#!/bin/sh\necho \"gh $*\" >> \"$HOME/gh.log\"\nexit 1\n" }
    // also { "raw": "...", "mode": "600" } and { "json": ..., "mode": "644" }
  },
  "compareConfigFileAfter": true,         // compare config file bytes after the run (migrations)
  "normalizeGuids": false,                // default true: each distinct UUID v4 in compared
                                          // files becomes <GUID#n> (v1 migration mints random ids)
  "compareFilesAfter": [".claude/settings.json"],  // more files to compare (missing == missing)
  "compareHomeTree": false,               // default true: every file under HOME, path + content
                                          // (catches stray writes like settings.json.bak)
  "homeTreeIgnore": [".cache/"],          // path prefixes skipped by compareHomeTree (default)
  "compareStderr": true                   // stderr is shown in diffs but compared only if set
}
```

`${CASE_DIR}` and `${HOME}` are substituted in every string of `stdin`, `stdinRaw`,
`config`, `configRaw`, `env`, `files` (keys too), `args`, `cwd`, `pathPrepend` and
`compareFilesAfter`. Stdin is always a pipe (empty when neither `stdin` nor
`stdinRaw` is given); the interactive TUI path is not covered.

## Fixtures

`fixtures/*.sh <dir>` build deterministic git repos (fixed identities and dates, no
user or system git config), always in place, so linked worktrees keep valid paths.
New fixtures can source `fixtures/_lib.sh` (`g`, `commit`, `target_dir`), or
`fixtures/_gitlib.sh` (`g`, `commit`, same conventions), which is self-contained so a
fixture works from a worktree's `fixtures/` dir too. The fixtures from `no-git` to
`git-detached` below source `_lib.sh`; the rest source `_gitlib.sh`.

| Fixture | Contents |
|---|---|
| `no-git` | plain directory, not in a repo |
| `git-clean` | one commit, clean, no remotes |
| `git-empty` | `git init`, no commits, one untracked file |
| `git-dirty` | staged (modify, add, delete) + unstaged + untracked changes |
| `git-ahead-behind` | `main` ahead 2 / behind 1 of `origin/main`; origin `https://github.com/acme/widgets.git`, upstream `https://github.com/upstream-org/widgets.git` |
| `git-worktree` | repo at `main/`, linked worktree at `linked/` on `feature/wt` |
| `git-conflict` | unmerged merge, two conflicted files |
| `git-detached` | detached HEAD |
| `git-conflict-spaces` | unmerged merge, three conflicted files, one named with runs of spaces (`a  b.txt`) |
| `git-filters` | modified files under a repository-defined filter driver that would touch `$HOME/FILTER-RAN` if run, an lfs filter and a config include; status and diff must not run the driver |
| `git-renames` | staged rename, copy-like add and type change, unstaged delete, names with spaces and non-ASCII, untracked files |
| `git-review` | clean repo on `main`, origin `https://github.com/acme/widgets.git`; also writes outside `<dir>`: a symlink under `$HOME/.cache/ccstatusline/git-review/` (the git-review cache file for this cwd and branch) to `<dir>.review.json`, so a case supplies the cache content with `files` (missing = no cache) |
| `git-review-repo` | clean repo on `feature/login`, origin `https://github.com/acme/widgets.git`, no git-review cache (for `--internal-refresh-git-review-cache` cases) |
| `git-symlink` | repo at `real/` with a modified file in `sub/`, and a symlink `link` -> `real/sub` (cwd through a symlink) |
| `git-tracking` | no `upstream` remote; `main` tracks `my/remote/main` while remotes `my` and `my/remote` both exist (longest remote name wins) |

## Notes on the corpus

- `gen/seed.mjs` reads the widget list from upstream `src/utils/widget-manifest.ts`
  and the powerline themes from `src/utils/colors.ts` (`upstream-src/v2.2.32`).
- No case has a `transcript_path` yet, so transcript-derived widgets (tokens-total,
  tokens-cached, the speed widgets, session-name, compaction-counter) only cover
  their no-transcript output, which for most of them is an empty line.
- An empty status line prints nothing at all, so 146 cases expect empty stdout
  (hidden or null widgets, `{}` stdin); they still check exit code and HOME writes.
- jj is not on the harness PATH, so the seed jj cases cover only "no jj"; fake one
  with `pathPrepend`. gh/glab are only used by background refreshes (off) and by the
  `--internal-refresh-git-review-cache` mode.
- Usage widgets without `rate_limits` take the API path, which ends at
  "no credentials" (no keychain, no `.credentials.json`, no network).
- The scratch path appears in outputs (cwd widgets, stderr), so reports from two
  harness runs differ in those paths; within one run both sides share them.

## The editor (TUI) harness

`tui.mjs` checks the interactive config editor. Each case in `tui-corpus/` runs upstream
and the Rust binary on a pseudo-terminal of the same size (`tui/pty-run.py`, Python's
stdlib `pty`), sends the same keys, and reads both terminals through
[`@xterm/headless`](https://www.npmjs.com/package/@xterm/headless) (pinned in
`tui/package.json`). After the start and after every key it compares the screen line
by line: the characters (trailing blanks trimmed), and, for every cell that shows a
character, a background color, inverse or underline, its foreground and background
color, bold, dim, italic, underline, inverse and strikethrough. Attributes on any other
blank cell (bold or strikethrough on a space) draw nothing and are not compared. After exit it compares the exit codes and every
file under HOME (`.cache/` excluded), byte for byte.

```sh
npm ci --prefix parity/tui --ignore-scripts      # once
node parity/tui.mjs                              # all cases vs target/release/ccstatusline-rs
node parity/tui.mjs --filter shell/configs --show-diff
node parity/tui.mjs --filter star --upstream-only   # print upstream's screens
```

The two sides run one after the other in the same HOME path, since paths can show on
screen. A step ends once upstream has written nothing for 700 ms (1.5 s at start); the
Rust side then gets up to 3 s to reach upstream's screen for that step, so a stall on a
loaded machine is not a failure. A case that leaves the editor running ends with
Ctrl+C, a clean exit. The world is the one described above, plus
`TERM=xterm-256color`, and stubs for `open`, `xdg-open` and `gio` that succeed without
opening anything.

A case:

```jsonc
{
  "name": "invalid-json-ctrl-s-guard-yes",
  "size": [100, 40],                 // columns, rows (default 100x40)
  "config": "{ not json",            // ~/.config/ccstatusline/settings.json: JSON value or raw string
  "files": { ".claude/settings.json": { "json": { "statusLine": { "command": "ccstatusline" } } } },
  "env": { "TERM": "xterm" },        // added to (or, with null, removed from) the environment
  "args": ["--config", "${HOME}/custom.json"],
  "pathPrepend": ["bin"],            // dirs under HOME (or absolute) put first on PATH, for fakes made with
                                     // "files": {"bin/claude": {"executable": "#!/bin/sh\necho 2.1.97"}}
  "seed": 1,                         // CCSL_TEST_RANDOM_SEED (default 1): new widgets' ids, powerline colors
  "nowMs": 1792584000000,            // CCSL_TEST_NOW_MS, the clock both sides see (default 2026-10-21T12:00Z)
  "quietMs": 700,                    // how long upstream stays silent to end a step (default 700)
  "startQuietMs": 1500,              // the same for the start screen (default 1500)
  "keys": ["ctrl+s", "enter", { "text": "abc" }]
}
```

Key names: `up`, `down`, `left`, `right`, `ctrl+left`, `ctrl+right`, `enter`,
`escape`, `tab`, `backspace`, `delete`, `space`, `ctrl+c`, `ctrl+s`; `{"text": "..."}`
sends literal input in one chunk. The report goes to `parity/last-tui-report.json` (gitignored).
