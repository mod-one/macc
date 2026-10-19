.PHONY: fmt fmt-check lint test test-tool-runner test-tool-runner-real test-contract web-build web-ci all check check-generic check-version release-bump

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
test-tool-runner:
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
	@python3 -c 'import json, pathlib, re, sys, tomllib; \
ws = tomllib.loads(pathlib.Path("Cargo.toml").read_text())["workspace"]["package"]["version"]; \
aws = tomllib.loads(pathlib.Path("adapters/Cargo.toml").read_text())["workspace"]["package"]["version"]; \
web = json.loads(pathlib.Path("web/package.json").read_text())["version"]; \
changelog = pathlib.Path("CHANGELOG.md").read_text(); \
expected = sys.argv[1].lstrip("v") if len(sys.argv) > 1 and sys.argv[1] else ws; \
errors = []; \
ws != expected and errors.append(f"Cargo.toml ({ws}) != {expected}"); \
aws != expected and errors.append(f"adapters/Cargo.toml ({aws}) != {expected}"); \
web != expected and errors.append(f"web/package.json ({web}) != {expected}"); \
not re.search(rf"^## \[{re.escape(expected)}\]", changelog, re.M) and errors.append(f"CHANGELOG.md missing ## [{expected}]"); \
sys.exit("\n".join(errors)) if errors else print(f"Version check OK: {expected}")' "$(TAG_VERSION)"

release-bump:
	@./scripts/bump-version.sh "$(VERSION)"

check: fmt-check lint check-version test test-tool-runner web-ci check-generic test-contract
