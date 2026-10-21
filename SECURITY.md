# Security policy

## Reporting a vulnerability

Please report vulnerabilities privately, through GitHub's **Report a vulnerability** button on the
[Security tab](https://github.com/danielnc/ccstatusline-rs/security) of this repository, not in a public issue.

Examples of what counts: a way to make ccstatusline-rs send an OAuth token anywhere other than
`https://api.anthropic.com`, to write outside its config and cache directories, or to let text from a repository,
a session or a config file reach the terminal as control sequences other than colors and hyperlinks.

## How ccstatusline-rs handles credentials

The usage widgets need Claude Code's OAuth token only when the status line JSON from Claude Code lacks the numbers
they show. The token is read from the macOS keychain or `.credentials.json`, the same sources upstream ccstatusline
uses. It is held only in memory and passed to `curl` on stdin (never on the command line), which sends it only to
`https://api.anthropic.com/api/oauth/usage`. It is never written to a cache file or a log; caches store only a
truncated SHA-256 of it, as upstream does.

The clauth widgets and `CCSTATUSLINE_RS_USAGE_SOURCE=clauth` read only clauth's `status.json`, which holds no
secrets, and never touch the keychain or the network.

## Supported versions

Only the latest commit on `main` receives fixes. There are no releases.
