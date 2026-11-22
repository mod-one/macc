.PHONY: fmt fmt-check lint test test-tool-runner test-tool-runner-real test-o2-benchmark o2-benchmark test-contract web-build web-ci all check check-generic check-version release-bump

all: fmt lint check-version test test-tool-runner web-ci check-generic test-contract

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
test-tool-runner: export MACC_PROTOCOL_CLI := $(CURDIR)/target/debug/macc
test-tool-runner:
	cargo build -p macc-cli --locked
	bash automat/tests/test_usage_projection.sh
	bash automat/tests/test_prompt_literals.sh
	bash automat/tests/test_completion_evidence.sh
	bash automat/tests/test_terminal_payload_construction.sh
	bash automat/tests/test_precondition_markers.sh
	bash automat/tests/test_quota_classification.sh
	bash automat/tests/test_provider_unavailable_resume.sh
	bash automat/tests/test_continuation_prompt.sh
	bash automat/tests/test_codex_effort_args.sh
	bash automat/tests/test_codex_resume_failure.sh
	bash automat/tests/test_session_resume_flags.sh
	python3 automat/tests/test_session_isolation.py
	python3 automat/tests/test_o2_real_tools.py
	python3 automat/tests/test_o2_benchmark.py
	python3 automat/tests/test_o2_output_reduction.py
	python3 automat/tests/test_o2_memory_mcp.py
	python3 automat/tests/test_o2_provider_navigation.py

# Offline deterministic regression; the future adapter does not call an AI service.
test-o2-benchmark:
	python3 automat/tests/test_o2_benchmark.py
	python3 automat/tests/test_o2_output_reduction.py
	python3 automat/tests/test_o2_memory_mcp.py
	python3 automat/tests/test_o2_provider_navigation.py

# Plan by default. O2_BENCHMARK_ARGS must select reviewed settings and a new output.
o2-benchmark:
	python3 scripts/o2_benchmark.py $(O2_BENCHMARK_ARGS)

# Explicit opt-in only; O2_REAL_ARGS must select reviewed tool JSONs and a report.
test-tool-runner-real:
	python3 automat/tests/o2_real_tools.py $(O2_REAL_ARGS)

test-contract:
	cargo test -p macc-registry --test contract --locked

web-build:
	cd web && npm ci && npm run build

web-ci:
	cd web && npm ci && npm run lint && npm run test && npm run build

check-generic:
	@./scripts/check-ui-tool-transparency.sh

check-version:
	@python3 scripts/check-version.py "$${TAG_VERSION:-}"

release-bump:
	@./scripts/bump-version.sh "$(VERSION)"

check: fmt-check lint check-version test test-tool-runner web-ci check-generic test-contract
