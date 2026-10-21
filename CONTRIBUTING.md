# Contributing to ccstatusline-rs

## Issues welcome, pull requests not accepted

Bug reports are very welcome, especially **parity mismatches**: a case where ccstatusline-rs prints something
different from upstream ccstatusline for the same input. Please include:

- the status line JSON (stdin) and the config file, with anything private replaced,
- the environment variables that matter (`CCSTATUSLINE_WIDTH`, `TZ`, `LANG`, `CLAUDE_CONFIG_DIR`),
- what upstream printed and what ccstatusline-rs printed (`| cat -v` makes escape codes visible).

A parity case in the harness format (see `parity/README.md`) is the best possible report.

This repository does **not accept pull requests** from outside contributors. You are free to fork it under the MIT
License; the instructions below are for working on a fork.

## Building and testing

```bash
git clone https://github.com/danielnc/ccstatusline-rs && cd ccstatusline-rs
make check          # rustfmt, clippy and the offline test suite (stable Rust)
make parity         # the differential harness against pinned upstream; needs node 24 and git
pre-commit install  # optional: the gitleaks hook blocks commits that contain secrets
```

CI (`.github/workflows/ci.yml`) runs the tests on Linux and macOS, the minimum supported Rust version, coverage,
rustfmt and clippy, the parity harness on macOS, and a gitleaks scan of the full history. CodeQL and OpenSSF
Scorecard run as separate workflows.

Agents working on a fork: `AGENTS.md` has the project's rules in checklist form.

## Privacy and secrets

Never commit tokens, OAuth data, keychain values, real transcripts, `.env` files, real names, emails, hostnames,
home paths, or account and profile names from a real setup. Parity cases must be synthetic.

## Security issues

Please don't open a public issue for a vulnerability. Report it privately, as described in [SECURITY.md](SECURITY.md).

## License

MIT (see `LICENSE`).
