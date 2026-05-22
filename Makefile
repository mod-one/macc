.PHONY: fmt fmt-check lint test test-contract web-build web-ci all check check-generic

all: fmt lint test web-ci check-generic test-contract

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --locked -- -D warnings

test:
	cargo test --workspace --locked
	cd adapters && cargo test --workspace --locked

test-contract:
	cargo test -p macc-registry --test contract --locked

web-build:
	cd web && npm ci && npm run build

web-ci:
	cd web && npm ci && npm run lint && npm run test && npm run build

check-generic:
	@./scripts/check-ui-tool-transparency.sh

check: fmt-check lint test web-ci check-generic test-contract
