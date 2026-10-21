# Upstream review: ccstatusline v2.2.17 → v2.2.32

Notes from reading upstream [ccstatusline](https://github.com/sirmalloc/ccstatusline) while porting it, written in
October 2026 against v2.2.32. Paths like `src/utils/usage-fetch.ts:883` point into upstream's source at that tag;
`v2.2.17:` marks the older release. These are observations about behaviour under heavy use (dozens of concurrent
Claude Code sessions, transcripts of hundreds of MB, several accounts), offered in the hope they are useful
upstream. ccstatusline-rs reproduces upstream's output byte for byte, so where a behaviour is visible in the output
it keeps it, and changes only how the work is done.

## The transcript is parsed on every render

`renderMultipleLines` runs `getTranscriptAnalysis` whenever stdin carries `transcript_path`
(`src/ccstatusline.ts:112-122`), and that analysis always computes token metrics (`src/utils/jsonl-metrics.ts:669-672`),
whatever widgets are configured. Nothing is cached between renders, and every render is a new process, so each
repaint streams the whole file and runs `JSON.parse` on every line.

Since v2.2.17 the read became a single streamed pass in 1 MiB chunks (v2.2.28, v2.2.29), which bounds memory to
roughly the longest line plus the accumulators. CPU and I/O are still proportional to the file's size on every
repaint. The speed widgets additionally walk every record for `agentId` and stream every referenced subagent file
(`jsonl-metrics.ts:235-255,579-597`).

ccstatusline-rs keeps a small per-transcript cache (device, inode, offset and running totals) and parses only the
bytes appended since the last render. A cold rebuild streams the file under a time budget and resumes on the next
render.

## Usage data and multiple accounts

`prefetchUsageDataIfNeeded` (`src/utils/usage-prefetch.ts:221-241`) takes the 5-hour and 7-day numbers from the
`rate_limits` Claude Code puts on stdin, and calls `GET https://api.anthropic.com/api/oauth/usage` only for fields
stdin lacks. Claude Code omits a window until the session's first API response, and omits a window whose
`resets_at` is not in the future, which is what a not-yet-started weekly window looks like.

- **v2.2.17** read the token from the un-suffixed macOS keychain item `Claude Code-credentials` first
  (`v2.2.17:utils/usage-fetch.ts:336-344`), ignoring `CLAUDE_CONFIG_DIR`, and kept one `usage.json` cache with no
  account identity (`v2.2.17:utils/usage-fetch.ts:525-537`). With several accounts in use, a session whose stdin
  lacked a window could show another account's numbers.
- **v2.2.32** reads the `CLAUDE_CONFIG_DIR`-specific keychain item `Claude Code-credentials-<sha256[:8]>`, then
  `$CLAUDE_CONFIG_DIR/.credentials.json` (`src/utils/usage-fetch.ts:589-619`), and gates the cache on a token hash.
  The cache file and the fetch lock are still single global files (`usage-fetch.ts:27-29`), so sessions on different
  accounts evict each other's cache and one account's lock or 429 backoff shows `[Timeout]` or `[Rate limited]` on
  the others. When no credential identity is available, any account's cache is accepted (`usage-fetch.ts:290-297`).

## Other findings

Each item gives the trigger and the effect.

- **A `security` process per render on macOS.** Credentials are resolved before the file cache is checked
  (`usage-fetch.ts:883` before `:888`), so any usage widget whose field is missing from stdin spawns
  `security find-generic-password` on every render. `fable-weekly-usage` and the extra-usage widgets can never be
  served from stdin. Upstream [#740](https://github.com/sirmalloc/ccstatusline/issues/740) reports this wedging
  `securityd`.
- **The usage fetch lock is check-then-write.** The in-flight lock is written with a plain `writeFileSync` after a
  separate check (`usage-fetch.ts:912-923`), so many renders starting in the same second can all call the API, and a
  resulting 429 blocks every account for the backoff period (300 s by default).
- **The 5-hour block fallback reads whole files.** `block-timer`, and `reset-timer` / `session-usage` without a reset
  time, scan every `projects/**/*.jsonl` modified in the last 10–48 hours, each read in full
  (`src/utils/jsonl-blocks.ts:44-85,183-222`). The result is cached per config directory, so tools that give each
  session its own `CLAUDE_CONFIG_DIR` pay for a scan per session. See also
  [#571](https://github.com/sirmalloc/ccstatusline/issues/571).
- **Non-atomic cache writes.** `usage.json`, `usage.lock`, the block caches and `claude-status.json` are rewritten in
  place; a concurrent reader can see a truncated file and treat it as a miss. The git cache uses a fixed temp file
  name (`src/utils/git.ts:416-434`), so two writers can interleave.
- **Waiting for stdin EOF forever.** `readStdin` (`src/ccstatusline.ts:51-78`) never returns if the writer keeps the
  pipe open, leaving processes behind ([#485](https://github.com/sirmalloc/ccstatusline/issues/485)).
  ccstatusline-rs stops once a complete JSON value has arrived and the pipe has been quiet for 100 ms.
- **Width probe subprocesses on macOS.** Without `CCSTATUSLINE_WIDTH`, each render walks up to eight ancestors with
  `ps` and `stty` (`src/utils/terminal.ts:40-82`); the subprocess-free probe is Linux-only. ccstatusline-rs uses
  `proc_pidinfo` and `TIOCGWINSZ` on macOS.
- **Claude Status has no in-flight lock.** When its 300 s cache expires, every concurrent render fetches
  `status.claude.com` (`src/utils/claude-service-status.ts:365-410`).
- **Config and Claude settings written on the render path.** While an update message is counting down
  (`settings.updatemessage.remaining > 0`), each render rewrites the config and re-syncs widget hooks into Claude's
  `settings.json` with a truncating write (`src/ccstatusline.ts:229-254`, `src/utils/hooks.ts:101-135`).
- **Token and speed totals over-count** ([#549](https://github.com/sirmalloc/ccstatusline/issues/549),
  [#643](https://github.com/sirmalloc/ccstatusline/issues/643)): Claude Code writes one transcript entry per content
  block with the same `usage`, and each is summed (`jsonl-metrics.ts:179-209`). Parity means ccstatusline-rs
  reproduces this.
- **Characters outside the Basic Multilingual Plane become U+FFFD** where a single UTF-16 unit is taken as a
  character: `src/widgets/CurrentWorkingDir.tsx:232,235` (fish style), `src/widgets/VimMode.ts:39`, and output
  truncation in `src/utils/custom-command-capture.ts:57`, which can split a surrogate pair. Reproduced for parity.
- **Session duration** uses the first and last valid timestamps in file order (`jsonl-metrics.ts:556`), not the
  minimum and maximum, so out-of-order lines can shorten or invert it.
- **A huge `rate_limits.resets_at`** (above about 8.64e12 seconds) throws `RangeError: Invalid time value` and exits
  1 (`src/utils/usage-prefetch.ts:173-178`). ccstatusline-rs renders instead.

## Git and jj widgets

- **Uncached spawns on warm renders.** Every `git-review` / `git-ci-status` call resolves its cache key with
  `git symbolic-ref --short HEAD` and more, uncached (`src/utils/git-review-cache.ts:623,743,763`), and every jj
  widget runs `jj root` again (`src/widgets/shared/jj-widget-base.ts:53`). In a test with `git-branch`,
  `git-review` and `git-ci-status`, a warm upstream render spawned 6 processes; ccstatusline-rs spawns none warm by
  prefetching once per render, folding the `rev-parse` queries into one process, and reading `HEAD` natively where
  the answer cannot differ from git's.
- **gh runs twice on branches without a PR.** With checks enabled, the error from `execFileSync` includes the whole
  command line, so any `gh` failure, including "no pull requests found", is read as "CI field unavailable" and `gh`
  runs again (`src/utils/git-review-cache.ts:443,455,515`).
- **A review cache entry with `state` or `title` missing throws** (`src/widgets/GitPr.ts:88,93`), which empties the
  whole status line; an unknown `checks.state` renders as `undefined0` (`src/widgets/GitCiStatus.ts:63`).
- **The conflict count collapses runs of spaces** in file names (`src/utils/git.ts:708`), so `a  b.txt` and
  `a b.txt` count as one file.
- **`git-worktree` prints `undefined/worktrees/`** when `rev-parse` prints a single line
  (`src/widgets/GitWorktree.ts:68`).
- **`jj-changes` and `jj-insertions` / `jj-deletions` can disagree**: they parse the diff stat with different
  regular expressions (`src/widgets/JjChanges.ts:41` against `src/utils/jj.ts:39`).

## Related open upstream issues

| Issue | Topic |
|---|---|
| [#740](https://github.com/sirmalloc/ccstatusline/issues/740) | per-render keychain reads on macOS |
| [#571](https://github.com/sirmalloc/ccstatusline/issues/571) | per-render costs: block scan, git commands |
| [#485](https://github.com/sirmalloc/ccstatusline/issues/485) | processes pile up when stdin stays open |
| [#397](https://github.com/sirmalloc/ccstatusline/issues/397) | CPU use with many concurrent sessions |
| [#560](https://github.com/sirmalloc/ccstatusline/issues/560) | stdin `rate_limits` preferred over the live usage API |
| [#521](https://github.com/sirmalloc/ccstatusline/issues/521) | `[No credentials]` with several keychain items |
| [#549](https://github.com/sirmalloc/ccstatusline/issues/549), [#643](https://github.com/sirmalloc/ccstatusline/issues/643) | token and speed over-counting |
