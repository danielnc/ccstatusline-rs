# ccstatusline-rs

**A native Rust port of [ccstatusline](https://github.com/sirmalloc/ccstatusline) by Matthew Breedlove
([@sirmalloc](https://github.com/sirmalloc)): the same Claude Code status line, with the same config file and
byte-identical output, at a fraction of the cost per render.**

[![CI](https://github.com/danielnc/ccstatusline-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/danielnc/ccstatusline-rs/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/danielnc/ccstatusline-rs/badge)](https://scorecard.dev/viewer/?uri=github.com/danielnc/ccstatusline-rs)
[![Codecov](https://codecov.io/gh/danielnc/ccstatusline-rs/graph/badge.svg)](https://codecov.io/gh/danielnc/ccstatusline-rs)
[![CodeRabbit reviews](https://img.shields.io/coderabbit/prs/github/danielnc/ccstatusline-rs?label=CodeRabbit%20reviews)](https://coderabbit.ai)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

It targets **ccstatusline v2.2.32**. This is an unofficial port, not affiliated with or endorsed by the upstream
project. All the design, the widgets and the config format are upstream's work; please star and support
[ccstatusline](https://github.com/sirmalloc/ccstatusline).

## Why

Claude Code runs the status line command on every refresh, one short-lived process per render. With one session
that is fine. With dozens of sessions open at once, the per-render cost of a Node process adds up: upstream starts a
Node runtime each time, and when `transcript_path` is set it streams and parses the whole session transcript on
every render, which for a long session is hundreds of MB.

ccstatusline-rs keeps upstream's behaviour and output and changes how the work is done:

- **Transcripts are read incrementally.** A small per-transcript cache (device, inode, offset, running totals)
  means each render parses only the lines appended since the last one. A cold start streams the file under a
  time budget and resumes on the next render; until then widgets fall back to the numbers Claude Code sends on stdin,
  as upstream does without a transcript.
- **No network on the render path.** Usage and service-status data are refreshed by one detached, lock-guarded
  background process; renders read caches. Caches and locks are per account, written atomically.
- **No subprocesses where upstream spawns them**: terminal width (`ps`/`stty`), free memory (`vm_stat`) and
  the macOS keychain lookup on warm renders are done natively or skipped.
- **It never hangs on an open stdin pipe** (upstream
  [#485](https://github.com/sirmalloc/ccstatusline/issues/485)): it stops once a complete JSON value has arrived and
  the pipe has been quiet for 100 ms.

### Measured

On one Apple Silicon MacBook Pro, 21 October 2026, with about 60 other agent sessions running (so timings are noisy):
the author's three-line powerline config (model, context and token widgets; usage and reset timers; git branch,
root and changes), a realistic stdin payload, and a 466 MB real session transcript. Peak RSS and wall time are the
median of 5 runs, from `/usr/bin/time -l`; `tools/bench.sh` reproduces them.

| Render | Peak RSS | Wall time |
|---|---|---|
| upstream 2.2.17, `transcript_path` stripped (the author's previous workaround) | 107 MB | 0.16 s |
| upstream 2.2.32, full payload with the 466 MB transcript | 281 MB | 0.88 s |
| ccstatusline-rs, first render on that transcript (150 ms indexing budget) | 6.3 MB | 0.16 s |
| ccstatusline-rs, warm | 2.6 MB | about 3 ms |
| ccstatusline-rs, 60 warm renders started at once | 2.9 MB each, 157 MB total | 0.05 s for all 60 |

The first render indexes as much of a new transcript as fits in its 150 ms budget and saves its place; the next
renders finish the job (a few renders for a file this size), after which each render parses only appended lines. Under
2.2.17, which read the whole transcript into memory several times per render, the author saw renders of about 1 GB
each while dozens of sessions refreshed at once, which is what prompted this port.

## Parity

For the same stdin JSON, config file and environment, ccstatusline-rs prints byte-identical output to upstream
v2.2.32, and writes the same files (config migrations, default config, hook settings). This is checked by a
differential harness (`parity/`) that runs the pinned upstream release under Node and the Rust binary on the same
inputs, with a fixed clock and no network, and compares stdout, exit codes and every file written under `HOME`.

Current result: **3103 of 3103 cases pass** (macOS, Node 24, October 2026). The corpus covers all 90 upstream widget
types (9 to 228 cases each, across their display modes, raw values, hide states, labels and number formats), git
repositories in clean, dirty, conflicted, detached, empty and worktree states (jj only without a jj binary), transcripts with subagents,
compactions and malformed lines, powerline themes at every color level, flex modes and truncation, config
migrations from v1 to v4, invalid configs, and CLI edge cases.

Known gaps:

- **The interactive config editor (TUI) is not ported yet.** Run without piped input, ccstatusline-rs only prints a
  note. Keep editing with `npx ccstatusline@latest`: the config file is shared, so both read and write the same
  `~/.config/ccstatusline/settings.json`.
- Reset times shown in a time zone other than UTC are always formatted with en-US conventions, whatever locale is
  configured; UTC output is exact for every locale.
- Only ISO-8601 timestamps are parsed in transcripts (upstream's `Date.parse` also accepts other forms, which
  Claude Code does not write).
- Where avoiding a stall or a wrong account is the point, output differs on purpose and never under the harness:
  with an expired usage cache a render shows the stale value at once instead of blocking up to 5 s on the fetch,
  and one account's rate-limit lock no longer shows `[Rate limited]` on another account's sessions.

## Install

From source, with a stable Rust toolchain (1.88 or newer):

```bash
cargo install --git https://github.com/danielnc/ccstatusline-rs
# or
git clone https://github.com/danielnc/ccstatusline-rs && cd ccstatusline-rs && cargo build --release
```

Then point Claude Code's status line at it in `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "ccstatusline-rs",
    "padding": 0
  }
}
```

It reads `~/.config/ccstatusline/settings.json` by default, or the file given with `--config <path>`, exactly as
upstream does. `--hook`, `--version` and `CCSTATUSLINE_WIDTH` behave as upstream. To go back, set the command to
`npx -y ccstatusline@latest` again; the config needs no change.

## Extras beyond upstream

Each extra is opt-in, behind its own widget type or environment variable, so parity holds when it is off. Widget
types that upstream does not know are kept by upstream's editor and render nothing there.

- **clauth widgets.** [clauth](https://github.com/uwuclxdy/clauth) is a multi-account manager for Claude Code; its
  daemon publishes per-account usage windows in `~/.clauth/status.json`. These widgets map the session's
  `CLAUDE_CONFIG_DIR` to its clauth profile and read that file, with no network or keychain access:
  - `clauth-session-usage`, `clauth-weekly-usage`, `clauth-weekly-sonnet-usage`, `clauth-weekly-opus-usage`,
    `clauth-fable-weekly-usage`, `clauth-reset-timer`, `clauth-weekly-reset-timer`: drop-in counterparts of the
    upstream widgets, with the same options, for the session's own account.
  - `clauth-profile` (the profile name, optionally with tier and chain slot), `clauth-account` (one compact
    `5h · 7d` summary) and `clauth-chain` (fallback-chain position, next candidate, pending switch).
  - They render nothing when the file is missing or stale, or the session is not on a clauth profile.
- **`CCSTATUSLINE_RS_USAGE_SOURCE=clauth`.** With this set, the *upstream* usage widgets (`session-usage`,
  `weekly-usage`, the reset timers, the per-model weekly widgets) fill whatever stdin `rate_limits` lack from the
  session's clauth profile instead of the API and keychain. This avoids showing another account's numbers when
  several accounts are in use, without changing the config.
- **herdr widgets.** In a [herdr](https://herdr.dev) pane, `herdr-tab`, `herdr-pane` and `herdr-workspace` show the
  tab, pane or workspace label (or its id), read from herdr's socket. They render nothing outside herdr.

## Upstream findings

Porting meant reading upstream closely, and a few behaviours matter under heavy use. They are written up, with
file and line references and links to the matching upstream issues, in
[docs/upstream-review.md](docs/upstream-review.md). In short:

- the whole transcript is parsed on every render whenever `transcript_path` is set;
- on macOS, usage widgets whose data is missing from stdin spawn a keychain lookup on every render
  ([#740](https://github.com/sirmalloc/ccstatusline/issues/740));
- the usage cache and fetch lock are shared by all accounts, and the lock is not atomic;
- the 5-hour block fallback reads whole transcripts, per config directory
  ([#571](https://github.com/sirmalloc/ccstatusline/issues/571));
- the process waits forever if the stdin pipe stays open
  ([#485](https://github.com/sirmalloc/ccstatusline/issues/485)).

## Running the parity harness

Prerequisites: Rust, Node 24 (Node's ICU decides upstream's text widths) and git.

```bash
make parity                                        # build, install pinned upstream, run every case
node parity/run.mjs --tag git-branch --show-diff   # one widget type, with diffs on failure
```

`parity/README.md` describes the case format and the environment both sides see.

## License

MIT, see [LICENSE](LICENSE). ccstatusline-rs is a derivative work of ccstatusline, and upstream's copyright notice is
kept.
