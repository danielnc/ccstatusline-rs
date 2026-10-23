.PHONY: check fmt parity check-secrets

# rustfmt, clippy and the offline test suite; what CI's test and lint jobs run
check:
	cargo fmt --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked

fmt:
	cargo fmt

# the differential harness against pinned upstream ccstatusline (needs node 24); what CI's parity job runs
parity:
	cargo build --release --locked
	npm ci --prefix parity/upstream --ignore-scripts
	node parity/run.mjs -q

# gitleaks over the full history; what CI's gitleaks job runs
check-secrets:
	gitleaks detect --source . --redact --log-opts="--all" --config .gitleaks.toml
