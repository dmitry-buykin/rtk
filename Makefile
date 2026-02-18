.PHONY: help check-rust-fast check-rust-strict check-rust-security test-rust-fast

help:
	@echo "Available targets:"
	@echo "  make check-rust-fast      # cargo check across workspace/targets"
	@echo "  make check-rust-strict    # clippy with -D warnings"
	@echo "  make check-rust-security  # cargo-deny full policy checks"
	@echo "  make test-rust-fast       # nextest test run"

check-rust-fast:
	cargo check --workspace --all-targets

check-rust-strict:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

check-rust-security:
	cargo deny check advisories bans licenses sources

test-rust-fast:
	cargo nextest run
