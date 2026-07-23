.PHONY: build check clippy fmt test bench bench-anchor compare all

build:
	cargo build-sbf

check:
	cargo check

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

fmt:
	cargo fmt; cargo +nightly fmt --all

test:
	cargo test

bench:
	cargo bench --bench initialize_ix_bench && cargo bench --bench deposit_ix_bench && cargo bench --bench swap_ix_bench && cargo bench --bench withdraw_ix_bench

bench-anchor:
	cargo bench --bench initialize_anchor_bench && cargo bench --bench deposit_anchor_bench && cargo bench --bench swap_anchor_bench && cargo bench --bench withdraw_anchor_bench

compare:
	cargo bench --bench compare_bench

all: fmt check clippy build test bench bench-anchor compare