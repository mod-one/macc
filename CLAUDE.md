# MACC — Claude Code working notes

## Rate-limit handling (RL series)

MACC implements automatic provider rate-limit handling. Key concepts:

### Error codes (E600 series)
- **E601** — Transient rate-limit (HTTP 429 / Claude 529 overloaded). Auto-retried with exponential backoff.
- **E602** — Hard quota exhaustion (monthly cap, budget limit). **Not retried** — pauses coordinator, requires operator action.
- **E603** — Session conflict (session ID reuse). Auto-retried.

### Canonical error model
- Each adapter (claude, codex, gemini) has an `ErrorNormalizer` in `core/src/coordinator/normalizers.rs`.
- Normalizers map stderr/exit-code → `CanonicalClass` → E-code via `canonical_to_error_code()`.
- `CanonicalClass::RateLimit` and `CanonicalClass::Overloaded` → E601 (retryable).
- `CanonicalClass::QuotaExhausted` → E602 (not retryable, `user_action_required=true`).
- `CanonicalClass::SessionConflict` → E603 (retryable).

### Default retry list
`E101,E102,E103,E301,E302,E303,E601,E603` — E602 excluded intentionally.

### Rate-limit config fields (in `automation.coordinator`)
| Field | Default | Description |
|---|---|---|
| `rate_limit_backoff_base_seconds` | 60 | Minimum backoff on first E601 |
| `rate_limit_backoff_max_seconds` | 3600 | Backoff cap |
| `rate_limit_fallback_enabled` | true | Dispatch to next tool when primary is throttled |
| `rate_limit_throttle_parallel` | true | Reduce effective_max_parallel on E601 |

### Key source files
- `core/src/coordinator/error_normalizer.rs` — `CanonicalClass`, `ToolError`, `ErrorNormalizer` trait
- `core/src/coordinator/normalizers.rs` — Claude/Codex/Gemini normalizer impls
- `core/src/coordinator/rate_limit.rs` — backoff computation, `ToolThrottleState`, throttle registry
- `core/src/coordinator/control_plane.rs` — wires normalization → backoff → events
- `tui/src/state.rs` — `ThrottledToolInfo`, `coordinator_throttled_tools`, TUI automation fields 26-29
- `tui/src/lib.rs` — CoordinatorLive rate-limit display, E602 pause overlay
- `core/src/service/coordinator_workflow.rs` — `ThrottledToolStatus`, `get_coordinator_status()`
- `cli/src/commands/web.rs` — `ApiThrottledToolStatus`, Web API exposure
