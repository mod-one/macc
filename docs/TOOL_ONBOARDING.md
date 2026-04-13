# Add a Tool End-to-End

This is the canonical, unified guide for integrating a new tool into MACC
without touching generic CLI/TUI/core flows.

## 1) Add ToolSpec

Create a new file in `registry/tools.d/<tool-id>.tool.yaml`:

- metadata (`id`, `display_name`, `description`)
- `fields` for tool settings exposed in TUI
- `doctor` checks
- `performer.runner` path
- optional `install` and `post_install` commands

Reference: `TOOLSPEC.md`.

Example:

```yaml
api_version: v1
id: example-tool
display_name: Example Tool
description: Workspace settings for the Example Tool assistant.
fields:
  - id: model
    label: Model
    kind:
      type: enum
      options: [fast, smart, turbo]
    help: Select the model preset.
    pointer: /tools/config/example-tool/model
  - id: enable_telemetry
    label: Enable Telemetry
    kind:
      type: bool
    help: Opt-in to anonymous telemetry.
    pointer: /tools/config/example-tool/telemetry
doctor:
  - kind: which
    value: example-tool-cli
    severity: warning
performer:
  runner: adapters/example/example.performer.sh
```

## 2) Add Performer Runner

Add runner script under adapter path, for example:

- `adapters/<tool>/<tool>.performer.sh`

Requirements:

- consume `--prompt-file`, `--tool-json`, task/worktree context args
- honor retry strategy and model escalation policy if relevant
- update session lease state via `.macc/state/tool-sessions.json` contract

## 3) Add Adapter Crate (if needed)

If the tool requires apply-time file generation:

1. create `adapters/<tool>/`
2. implement adapter logic
3. wire crate in `registry/Cargo.toml` so registry discovers it

If only runtime performer behavior changes, adapter crate changes may not be required.

Minimal adapter sketch:

```rust
pub struct ExampleToolAdapter;

impl ToolAdapter for ExampleToolAdapter {
    fn id(&self) -> String {
        "example-tool".to_string()
    }

    fn plan(&self, ctx: &PlanningContext) -> macc_core::Result<ActionPlan> {
        let mut plan = ActionPlan::new();

        let model = ctx
            .resolved
            .tools
            .get_value("/tools/config/example-tool/model")
            .and_then(|v| v.as_str())
            .unwrap_or("fast");

        let content = format!("model = {}\n", model);
        plan.add_action(Action::WriteFile {
            path: ".example-tool/config".to_string(),
            content: content.into_bytes(),
            scope: Scope::Project,
        });

        Ok(plan)
    }
}
```

If you add an adapter crate, ensure it is wired in `registry/src/lib.rs` so the registry discovers it.

## 4) Error Normalization

Every tool adapter must provide a per-adapter error normalizer. Without this,
unknown tool failures collapse to `E901` and lose retry/user-action semantics.

Canonical contract source of truth:

- `core/src/coordinator/error_normalizer.rs`

### 4.1 CanonicalClass Reference

Map tool-native stderr/stdout patterns into `CanonicalClass`, then derive the
final E-code with `canonical_to_error_code`.

| CanonicalClass | Operational meaning | Typical tool output pattern | E-code |
| --- | --- | --- | --- |
| `Auth` | Invalid/expired credential. | `invalid_api_key`, `UNAUTHENTICATED`, auth error text | `E201` |
| `Billing` | Account billing/payment issue. | `payment required`, `billing`, suspended account | `E201` |
| `RateLimit` | Transient request throttling. | `429`, `too many requests`, `rate_limit_error` | `E601` |
| `QuotaExhausted` | Hard budget/usage cap reached. | `insufficient_quota`, `usage limit`, `RESOURCE_EXHAUSTED` + quota keywords | `E602` |
| `Overloaded` | Provider capacity issue. | `529`, `503`, `overloaded`, temporary unavailable | `E601` |
| `Timeout` | Request/connection timed out. | `timeout`, `DEADLINE_EXCEEDED`, `timed out` | `E101` |
| `SessionConflict` | Session ID collision/reuse. | `session already in use`, session conflict text | `E603` |
| `ToolNotFound` | Tool binary/model/endpoint missing. | `command not found`, `NOT_FOUND`, `404` | `E102` |
| `OutputMalformed` | Output could not be parsed. | malformed JSON/protocol output, `INVALID_ARGUMENT` | `E103` |
| `Network` | DNS/TLS/socket/connectivity error. | `ECONNREFUSED`, `ECONNRESET`, DNS failure | `E101` |
| `GitConflict` | Worktree/git branch conflict. | branch conflict during setup/merge | `E304` |
| `PolicyViolation` | Permission/safety/policy denial. | `PERMISSION_DENIED`, permission/policy error | `E201` |
| `PostCommitFailure` | Failure after merge/commit phase. | post-merge validation/reconciliation failure | `E504` |
| `Internal` | Provider internal failure. | `500`, internal server error | `E901` |
| `Unknown` | Could not classify pattern. | non-empty failure output with no match | `E901` |

### 4.2 Required Adapter Error Sub-Codes

Before submitting a new adapter, validate these coordinator-relevant sub-codes:

| Code | What adapter authors must ensure |
| --- | --- |
| `E101` | Timeout/network/transient failures map to `Timeout` or `Network`. |
| `E102` | Missing CLI/model/endpoint failures map to `ToolNotFound`. |
| `E103` | Malformed/unparseable output maps to `OutputMalformed`. |
| `E104` | Partial-change performer failures are preserved as coordinator internal sub-code (do not mask with broad `Unknown`). |
| `E105` | Non-zero exit after apparent completion is preserved as coordinator internal sub-code (do not mask with broad `Unknown`). |
| `E601` | Rate-limit/overload failures map to `RateLimit` or `Overloaded`. |
| `E602` | Hard quota exhaustion maps to `QuotaExhausted`. |
| `E603` | Session collisions map to `SessionConflict`. |

Notes:

- `E101`, `E102`, `E103`, `E601`, `E602`, and `E603` come directly from your
  `CanonicalClass` mapping.
- `E104` and `E105` are coordinator internal sub-codes that still need coverage
  in adapter failure-path validation so your matcher does not hide them behind
  `Unknown`/`E901`.

### 4.3 Implement and Register a Normalizer

1. Create `adapters/<tool>/src/error_normalizer.rs`.
2. Implement `ErrorNormalizer`:

```rust
use macc_adapter_shared::error_normalizer::{ErrorNormalizer, ToolError};

pub struct ExampleErrorNormalizer;

impl ErrorNormalizer for ExampleErrorNormalizer {
    fn normalize(&self, exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError> {
        // Match provider-specific patterns in priority order.
        // Build ToolError with canonical_class + canonical_to_error_code(...).
        let _ = (exit_code, stderr, stdout);
        None
    }
}
```

3. Register the normalizer in `adapters/<tool>/src/lib.rs` using `inventory`:

```rust
inventory::submit! {
    macc_core::coordinator::error_normalizer::NormalizerRegistration {
        tool_id: "example-tool",
        factory: || Box::new(crate::error_normalizer::ExampleErrorNormalizer),
    }
}
```

4. Ensure your adapter crate is linked into the final binary, or the
   registration will not appear in `NormalizerRegistry::from_inventory()`.

### 4.4 Worked Example: 429 Rate Limit

Minimal expected flow:

1. Tool output contains `HTTP 429 Too Many Requests`.
2. Your normalizer matches the pattern and sets
   `canonical_class = CanonicalClass::RateLimit`.
3. Call `canonical_to_error_code(&CanonicalClass::RateLimit)` -> `E601`.
4. Coordinator consumes `E601` and applies rate-limit backoff behavior.

### 4.5 Pre-Submission Error Checklist

- Rate limit path: `429` / `too many requests` -> `RateLimit` -> `E601`.
- Auth failure path: invalid key/token -> `Auth` -> `E201`.
- Quota exhausted path: plan/usage cap wording -> `QuotaExhausted` -> `E602`.
- Tool crash path: non-zero exit with internal/tool crash text -> classified, not
  silently `Unknown` unless truly unclassifiable.
- Malformed output path: parse/format failure -> `OutputMalformed` -> `E103`.
- Session collision path: session reuse/in-use -> `SessionConflict` -> `E603`.
- Fallback path: unmatched non-empty failure output intentionally lands on
  `Unknown`/`E901`.

## 5) Validate Tool-Agnostic Guardrails

Do not hardcode vendor names in generic UX paths:

- CLI (`cli/`)
- TUI (`tui/`)
- generic core flows (`core/`)

Run:

```bash
make check-generic
```

## 6) Add/Update Tests

Minimum:

- ToolSpec contract coverage (`macc-registry` tests).
- Adapter behavior tests (if adapter changed).
- Automation integration tests if coordinator/performer behavior changed.

Run:

```bash
make test
make test-contract
./automat/tests/run.sh
```

## 7) Update Documentation

Update all relevant docs in same PR:

- `README.md` command/behavior changes
- `MACC.md` architecture/operational behavior
- `CHANGELOG.md` (`Unreleased`)
- optional tool-specific docs/examples

## 8) Release Readiness

Before release, ensure:

- CI green on all required jobs
- changelog entry exists
- compatibility policy still accurate

Reference: `docs/RELEASE.md`.

## Quick checklist

1. Add ToolSpec (`registry/tools.d/...`).
2. Add performer runner (`adapters/<tool>/...performer.sh`).
3. Add/adjust adapter crate and registry wiring if apply-time generation is needed.
4. Run guardrails/tests (`make check-generic`, `make test`, `make test-contract`, `./automat/tests/run.sh`).
5. Update docs and `CHANGELOG.md`.
