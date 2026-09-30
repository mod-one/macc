.PHONY: fmt fmt-check lint test test-tool-runner test-contract web-build web-ci all check check-generic

all: fmt lint test test-tool-runner web-ci check-generic test-contract

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --locked

test:
	cargo test --workspace --locked
	cd adapters && cargo test --workspace --locked

# Add runner regression scripts for other tools to this target.
test-tool-runner:
	bash automat/tests/test_precondition_markers.sh
	bash automat/tests/test_quota_classification.sh
	bash automat/tests/test_continuation_prompt.sh
	bash automat/tests/test_codex_effort_args.sh
	bash automat/tests/test_codex_resume_failure.sh
	bash automat/tests/test_session_resume_flags.sh

test-contract:
	cargo test -p macc-registry --test contract --locked

web-build:
	cd web && npm ci && npm run build

web-ci:
	cd web && npm ci && npm run lint && npm run test && npm run build

check-generic:
	@./scripts/check-ui-tool-transparency.sh

check: fmt-check lint test test-tool-runner web-ci check-generic test-contract
