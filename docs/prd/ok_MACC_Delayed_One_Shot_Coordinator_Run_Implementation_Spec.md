# MACC Delayed One-Shot Coordinator Run

## Implementation Specification

**Project:** MACC — Multi-Assistant Code Config  
**Feature:** Delayed one-shot execution of `macc coordinator run`  
**Status:** Ready for implementation  
**Scope:** CLI-only, process-bound delay  
**Target commands:**

```bash
macc coordinator run --in 30m
macc coordinator run --at "2026-06-28T02:00"
```

---

## 1. Executive summary

MACC already provides a native coordinator control plane capable of running the full coordinator cycle, selecting tasks, dispatching performers, supervising worktrees, advancing task states, reconciling results, handling rate limits, and performing cleanup.

This feature adds a lightweight way to delay the start of that existing coordinator run without introducing a scheduling subsystem.

The implementation must:

1. Add `--in <DURATION>` to start the coordinator after a relative delay.
2. Add `--at <DATETIME>` to start the coordinator at an absolute date and time.
3. Preserve the current behavior of `macc coordinator run` when neither option is supplied.
4. Reuse the existing coordinator run path after the delay expires.
5. Allow cancellation with `Ctrl+C` before the coordinator starts.
6. Revalidate project and coordinator preconditions when the delay expires.
7. Reject invalid, conflicting, or past scheduling values with clear errors.

This is intentionally a **delayed one-shot run**, not a persistent scheduler.

---

## 2. Context in MACC

The existing MACC coordinator is the primary automation control plane. Its normal full-cycle sequence is:

```text
sync -> dispatch -> advance -> reconcile -> cleanup
```

The current coordinator command model includes:

```bash
macc coordinator
macc coordinator run
macc coordinator dispatch
macc coordinator advance
macc coordinator resume
macc coordinator sync
macc coordinator sync-prd
macc coordinator audit-prd
macc coordinator status
macc coordinator reconcile
macc coordinator unlock
macc coordinator cleanup
```

The delayed execution feature must be implemented as a thin pre-run layer around the existing `run` action. It must not duplicate, fork, or partially reimplement coordinator behavior.

### 2.1 Architectural principle

The coordinator remains responsible for coordinator execution.

The delayed-run layer is responsible only for:

- parsing the requested delay or target time;
- resolving the exact start instant;
- displaying the pending run;
- waiting in a cancellation-safe manner;
- invoking the normal coordinator run path when the target time is reached.

### 2.2 Product value

This feature provides practical automation for cases such as:

- starting a long run later in the evening;
- waiting for a provider quota window to reset;
- delaying work until after another local process is expected to finish;
- starting a coordinator run during a known low-activity period;
- preparing a run now without adding OS scheduler or daemon complexity.

---

## 3. Goals

### 3.1 Functional goals

- Support relative delayed execution.
- Support absolute local or offset-aware date-time execution.
- Preserve all existing coordinator run flags.
- Use the same execution behavior as an immediate coordinator run.
- Allow clean cancellation before execution begins.
- Produce clear human-readable status output.
- Produce deterministic errors for invalid scheduling input.
- Keep the implementation small and isolated.

### 3.2 Engineering goals

- Avoid a second coordinator execution path.
- Keep scheduling logic independently testable.
- Separate pure time-resolution logic from async waiting and command execution.
- Avoid state changes before the scheduled run begins.
- Avoid unnecessary changes to `.macc/macc.yaml` or other persisted formats.
- Maintain Linux, Windows, and macOS compatibility.

---

## 4. Non-goals

The following capabilities are explicitly outside this feature:

- recurring schedules;
- cron expressions;
- persistent scheduled jobs;
- execution after terminal closure;
- execution after process termination;
- execution after reboot;
- a background daemon or service;
- OS scheduler integration such as cron, systemd timers, launchd, or Windows Task Scheduler;
- detached/background process management;
- schedule management commands such as `list`, `enable`, `disable`, or `remove`;
- storage of scheduled runs in `.macc/macc.yaml`;
- a scheduler database or scheduler state file;
- Web UI schedule management;
- TUI schedule management;
- remote triggering;
- changing coordinator retry, rate-limit, locking, or recovery policies.

The process must remain alive until the target time is reached.

---

## 5. User-facing CLI contract

## 5.1 Immediate run

The current behavior remains unchanged:

```bash
macc coordinator run
```

Expected result: the coordinator starts immediately.

## 5.2 Relative delayed run

```bash
macc coordinator run --in 30m
```

Additional valid examples:

```bash
macc coordinator run --in 45s
macc coordinator run --in 2h
macc coordinator run --in "1h 30m"
macc coordinator run --in 1d
```

The duration syntax should be delegated to a proven Rust duration parser rather than implemented manually.

Recommended parser:

```toml
humantime = "2"
```

If MACC already has an equivalent duration parser, reuse it instead of adding a dependency.

## 5.3 Absolute delayed run

Offset-aware timestamp:

```bash
macc coordinator run --at "2026-06-28T02:00:00+02:00"
```

UTC timestamp:

```bash
macc coordinator run --at "2026-06-28T00:00:00Z"
```

Local machine time without an explicit offset:

```bash
macc coordinator run --at "2026-06-28T02:00"
```

A date-time without an offset is interpreted in the machine's local timezone.

## 5.4 Combination with existing coordinator options

All existing options supported by `macc coordinator run` must continue to work:

```bash
macc coordinator run \
  --in 45m \
  --max-parallel 3 \
  --max-dispatch 10
```

```bash
macc coordinator run \
  --at "2026-06-28T02:00:00+02:00" \
  --prd prd.json \
  --tool-priority codex,claude,gemini
```

The scheduling flags only affect when the normal run begins. They must not modify coordinator options or config resolution.

## 5.5 Mutual exclusion

`--in` and `--at` must be mutually exclusive.

Invalid command:

```bash
macc coordinator run \
  --in 30m \
  --at "2026-06-28T02:00:00+02:00"
```

Expected CLI error:

```text
error: the argument '--in <DURATION>' cannot be used with '--at <DATETIME>'
```

This should be enforced by Clap at argument parsing time.

## 5.6 Scope of the flags

`--in` and `--at` apply only to the `run` action.

They must not be accepted for:

```bash
macc coordinator status
macc coordinator dispatch
macc coordinator advance
macc coordinator cleanup
macc coordinator reconcile
```

This avoids ambiguous semantics and keeps the feature narrowly scoped.

---

## 6. Date-time and duration semantics

## 6.1 Relative delay semantics

`--in <DURATION>` resolves the target instant as:

```text
target = current system time + parsed duration
```

Recommended rules:

- The duration must parse successfully.
- A zero duration is rejected.
- Negative durations are rejected by the parser.
- No artificial maximum duration is required.
- Very large durations may be rejected if they overflow the date-time representation.

Examples:

| Input | Result |
|---|---|
| `--in 30m` | Start approximately 30 minutes after command invocation |
| `--in 2h` | Start approximately 2 hours after command invocation |
| `--in 0s` | Validation error |
| `--in abc` | Parse error |

## 6.2 Absolute date-time semantics

The parser should accept the following forms, in this order:

1. RFC 3339 with offset:

   ```text
   2026-06-28T02:00:00+02:00
   2026-06-28T00:00:00Z
   ```

2. Local date-time without seconds:

   ```text
   2026-06-28T02:00
   ```

3. Local date-time with seconds:

   ```text
   2026-06-28T02:00:00
   ```

Recommended dependency:

```toml
chrono = { version = "0.4", features = ["clock"] }
```

If MACC already uses `chrono`, reuse the existing dependency and version.

## 6.3 Local timezone behavior

A timestamp without an explicit offset is interpreted using the machine's local timezone at command invocation.

Example:

```bash
macc coordinator run --at "2026-06-28T02:00"
```

On a machine configured for Europe/Paris, this means 02:00 in Europe/Paris local time.

### 6.3.1 Daylight-saving transitions

A local date-time can be invalid or ambiguous during a daylight-saving transition.

The implementation must not silently guess.

- If the local time does not exist, return an error.
- If the local time maps to two instants, return an error.
- Recommend supplying an explicit UTC offset.

Example error:

```text
error: local date-time '2026-10-25T02:30' is ambiguous in the current timezone; use an explicit offset such as '+01:00' or '+02:00'
```

This rule avoids hidden or platform-dependent execution times.

## 6.4 Past timestamps

A target time that is already in the past must be rejected.

Example:

```text
error: scheduled start time is in the past: 2026-06-04T02:00:00+02:00
```

MACC must not treat a past `--at` value as “run immediately,” because that could start expensive autonomous work unexpectedly after a typo or copied command.

## 6.5 Near-present timestamps

Time passes between parsing and validation. A small tolerance should avoid flaky behavior for values that become past during parsing or formatting.

Recommended policy:

- Capture `now` once during schedule resolution.
- Require `target > now`.
- Do not add a hidden grace period.
- In tests, use injected/frozen time instead of real near-present values.

## 6.6 Clock changes during waiting

The two scheduling modes should use different waiting semantics:

### `--in`

Relative delays should use a monotonic timer through Tokio:

```rust
 tokio::time::sleep(duration)
```

This prevents wall-clock adjustments from shortening or extending the requested relative delay unexpectedly.

### `--at`

Absolute target times are resolved against wall-clock time. Calculate the initial delay from the target timestamp, then wait using Tokio's monotonic timer.

This intentionally keeps the implementation simple. If the operating system clock is changed after scheduling, the already-resolved wait is not recalculated.

This limitation should be documented but does not require a clock-monitoring loop.

---

## 7. Runtime behavior

## 7.1 High-level flow

```text
Parse CLI arguments
        |
        v
Resolve optional delayed-start request
        |
        +-- no delay --> invoke normal coordinator run immediately
        |
        v
Validate target time
        |
        v
Print scheduled-run summary
        |
        v
Wait for target time or Ctrl+C
        |
        +-- Ctrl+C --> exit cleanly without starting coordinator
        |
        v
Revalidate project and coordinator preconditions
        |
        v
Invoke the existing coordinator run path
```

## 7.2 No coordinator initialization before waiting

Before the wait finishes, MACC must avoid coordinator-side mutations.

In particular, delayed-run setup must not:

- claim tasks;
- create worker worktrees;
- update the task registry;
- create coordinator runtime state;
- acquire the coordinator execution lock for the entire delay;
- open performer sessions;
- write delayed-run state files;
- emit coordinator-start events.

Reading enough project context to validate the command and display the project path is acceptable.

## 7.3 Revalidation after waiting

The repository may change while the command is waiting. Therefore, checks that affect actual execution must occur immediately before the coordinator starts.

The existing coordinator run path should remain the authority for these checks.

At minimum, the normal start path should verify:

- the project still exists and is accessible;
- the project is still a valid MACC project;
- the configured PRD can still be resolved;
- coordinator configuration can still be loaded;
- another coordinator run is not active;
- required registry/storage files can be accessed;
- existing run-specific prerequisites still pass.

Do not duplicate these checks in the delayed-run module. Invoke the same preflight/start path used by an immediate run.

## 7.4 Concurrent coordinator behavior

The delayed command must not hold the coordinator lock during the waiting period.

When the target time is reached, it must attempt to start normally. If another coordinator is active, the existing coordinator conflict/lock mechanism should reject the run.

Expected behavior:

```text
Scheduled start time reached.
Validating project and coordinator state...
error: another coordinator instance is already running
```

The delayed run must not:

- queue itself behind the active coordinator;
- wait indefinitely for the active coordinator to finish;
- terminate the active coordinator;
- start a second concurrent coordinator.

## 7.5 Run options snapshot

CLI options are parsed when the delayed command is created and retained in memory until execution.

Project configuration should be loaded through the existing run path at actual start time, not frozen for the entire delay.

This produces the following expected behavior:

- CLI overrides remain exactly as entered.
- Changes to `.macc/macc.yaml` made during the wait are visible when the run starts.
- Changes to PRD or repository state made during the wait are visible when the run starts.

---

## 8. Cancellation and signals

## 8.1 Ctrl+C before the run starts

The waiting phase must be interruptible with `Ctrl+C`.

Recommended Tokio implementation:

```rust
 tokio::select! {
     _ = tokio::time::sleep(delay) => {
         // Continue to the existing coordinator run path.
     }
     signal_result = tokio::signal::ctrl_c() => {
         signal_result?;
         return Ok(DelayedRunOutcome::Cancelled);
     }
 }
```

Expected output:

```text
Scheduled coordinator run cancelled.
```

Expected exit code: `0`.

Cancellation before execution is an intentional user action, not a coordinator failure.

## 8.2 Ctrl+C after the run starts

After the delay expires and the existing coordinator run path begins, signal behavior must be unchanged from the current coordinator implementation.

The delayed-run layer must not intercept or redefine coordinator shutdown semantics after handoff.

## 8.3 Platform behavior

Use Tokio's cross-platform signal API where available. The feature must work on:

- Linux;
- Windows;
- macOS.

No Unix-only signal handling should be required for the waiting phase.

---

## 9. User-visible output

## 9.1 Standard output for `--in`

Example:

```text
Coordinator run scheduled.
Project: /home/user/project
Starts at: 2026-06-28T02:00:00+02:00
Delay: 30m
Press Ctrl+C to cancel.
```

## 9.2 Standard output for `--at`

Example:

```text
Coordinator run scheduled.
Project: /home/user/project
Starts at: 2026-06-28T02:00:00+02:00
Delay: 3h 24m 12s
Press Ctrl+C to cancel.
```

## 9.3 Output when the target is reached

```text
Scheduled start time reached.
Validating project and coordinator state...
Starting coordinator run.
```

After this point, normal coordinator output takes over.

## 9.4 Quiet mode

MACC already defines a global quiet setting that suppresses non-essential output.

Recommended behavior under quiet mode:

- Suppress the multi-line schedule summary.
- Suppress countdown-related informational messages.
- Keep validation and runtime errors visible.
- Keep normal coordinator quiet-mode behavior unchanged.

Do not add a continuously updating countdown. It creates unnecessary terminal complexity, complicates logs, and is not needed for the first version.

## 9.5 Structured output compatibility

If the coordinator command currently supports a machine-readable or JSON mode, delayed-run status should not emit unstructured text that breaks the contract.

Preferred behavior:

- Reuse the existing output abstraction if one exists.
- Represent schedule creation, cancellation, and start as structured events in JSON mode.
- Do not invent a separate JSON protocol solely for this feature.

Example conceptual event:

```json
{
  "event": "coordinator_run_scheduled",
  "scheduled_at": "2026-06-28T02:00:00+02:00",
  "delay_seconds": 1800
}
```

This subsection applies only if a structured coordinator output mode already exists.

---

## 10. Error handling

## 10.1 Error categories

The delayed-run feature needs a small set of validation errors. These are CLI validation errors, not task execution errors such as E101, E601, or E602.

Recommended categories:

| Category | Example |
|---|---|
| Conflicting arguments | Both `--in` and `--at` supplied |
| Invalid duration | `--in abc` |
| Zero duration | `--in 0s` |
| Invalid date-time | `--at tomorrow-night` |
| Past date-time | Target precedes current time |
| Ambiguous local date-time | DST overlap |
| Nonexistent local date-time | DST gap |
| Date-time overflow | Relative duration exceeds supported range |
| Signal setup failure | Ctrl+C listener cannot be initialized |
| Coordinator start conflict | Another coordinator is already active at target time |

## 10.2 Recommended messages

### Invalid duration

```text
error: invalid value 'abc' for '--in <DURATION>': expected a duration such as '30m', '2h', or '1h 30m'
```

### Zero duration

```text
error: '--in' duration must be greater than zero; omit '--in' to run immediately
```

### Invalid absolute time

```text
error: invalid value '2026/06/28 02:00' for '--at <DATETIME>': expected RFC 3339 or local format 'YYYY-MM-DDTHH:MM[:SS]'
```

### Past time

```text
error: scheduled start time is in the past: 2026-06-04T02:00:00+02:00
```

### Ambiguous local time

```text
error: local date-time '2026-10-25T02:30' is ambiguous in the current timezone; provide an explicit UTC offset
```

### Nonexistent local time

```text
error: local date-time '2026-03-29T02:30' does not exist in the current timezone; provide a valid time or explicit UTC offset
```

## 10.3 Exit codes

Use MACC's existing CLI exit-code conventions.

Where no project-wide convention exists, recommended behavior is:

| Outcome | Exit code |
|---|---:|
| Delayed run completed successfully | `0` |
| User cancels before start | `0` |
| Invalid arguments or scheduling value | `2` through Clap where applicable |
| Coordinator start or runtime failure | Existing coordinator failure code |

Do not translate coordinator errors into new scheduler-specific exit codes.

---

## 11. Proposed Rust design

## 11.1 CLI argument model

The scheduling options should live directly on the coordinator `run` command argument structure.

Conceptual Clap definition:

```rust
use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct CoordinatorRunArgs {
    /// Start the coordinator after a relative delay, for example 30m or 2h.
    #[arg(long = "in", value_name = "DURATION", conflicts_with = "at")]
    pub run_in: Option<String>,

    /// Start the coordinator at an absolute date-time.
    #[arg(long, value_name = "DATETIME", conflicts_with = "run_in")]
    pub at: Option<String>,

    // Existing coordinator run options remain here or are flattened here.
}
```

Because `in` is a Rust keyword, use an internal field such as `run_in` while exposing the CLI flag as `--in`.

Depending on the current command hierarchy, `conflicts_with` must reference the Rust argument ID generated by Clap. Confirm the actual ID in tests.

An alternative is an `ArgGroup`, but two direct conflict declarations are sufficient and simpler.

## 11.2 Core scheduling types

Recommended module:

```text
cli/src/commands/coordinator/delayed_run.rs
```

If coordinator command code is organized differently, place the module next to the current `run` command handler. Do not move scheduling into the coordinator business-logic core unless another consumer needs it.

Recommended types:

```rust
use chrono::{DateTime, Local};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelayedStartRequest {
    After(Duration),
    At(String),
}

#[derive(Debug, Clone)]
pub struct ResolvedDelayedStart {
    pub scheduled_at: DateTime<Local>,
    pub delay: Duration,
    pub source: DelayedStartSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayedStartSource {
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayedRunOutcome {
    Ready,
    Cancelled,
}
```

The final implementation may simplify these types, but it should preserve the separation between:

- raw CLI input;
- resolved target time;
- async wait outcome.

## 11.3 Pure functions

Recommended functions:

```rust
pub fn parse_relative_delay(input: &str) -> Result<Duration, DelayedRunError>;

pub fn parse_absolute_time(
    input: &str,
    now: DateTime<Local>,
) -> Result<DateTime<Local>, DelayedRunError>;

pub fn resolve_delayed_start(
    run_in: Option<&str>,
    at: Option<&str>,
    now: DateTime<Local>,
) -> Result<Option<ResolvedDelayedStart>, DelayedRunError>;
```

These functions must not:

- sleep;
- access the repository;
- start the coordinator;
- write files;
- print output.

This makes parsing and time semantics straightforward to unit test.

## 11.4 Async wait function

Recommended function:

```rust
pub async fn wait_until_start(
    schedule: &ResolvedDelayedStart,
) -> Result<DelayedRunOutcome, DelayedRunError>;
```

Conceptual implementation:

```rust
pub async fn wait_until_start(
    schedule: &ResolvedDelayedStart,
) -> Result<DelayedRunOutcome, DelayedRunError> {
    tokio::select! {
        _ = tokio::time::sleep(schedule.delay) => Ok(DelayedRunOutcome::Ready),
        signal_result = tokio::signal::ctrl_c() => {
            signal_result.map_err(DelayedRunError::Signal)?;
            Ok(DelayedRunOutcome::Cancelled)
        }
    }
}
```

## 11.5 Command handler integration

Conceptual command handler:

```rust
pub async fn run_coordinator_command(
    context: &CommandContext,
    args: CoordinatorRunArgs,
) -> anyhow::Result<()> {
    let now = chrono::Local::now();
    let schedule = resolve_delayed_start(
        args.run_in.as_deref(),
        args.at.as_deref(),
        now,
    )?;

    if let Some(schedule) = schedule {
        context.output.print_delayed_run_summary(&schedule)?;

        match wait_until_start(&schedule).await? {
            DelayedRunOutcome::Cancelled => {
                context.output.print_delayed_run_cancelled()?;
                return Ok(());
            }
            DelayedRunOutcome::Ready => {
                context.output.print_delayed_run_ready()?;
            }
        }
    }

    // This must be the same existing function used for immediate runs.
    execute_coordinator_run(context, args.into_existing_run_args()).await
}
```

The actual function and type names should follow the current codebase.

The critical requirement is the final call:

```text
existing immediate-run function
```

There must not be separate `execute_immediate_run` and `execute_scheduled_run` coordinator implementations.

## 11.6 Error type

Recommended local error type:

```rust
#[derive(Debug, thiserror::Error)]
pub enum DelayedRunError {
    #[error("invalid delay '{input}': {reason}")]
    InvalidDuration { input: String, reason: String },

    #[error("'--in' duration must be greater than zero; omit '--in' to run immediately")]
    ZeroDuration,

    #[error("invalid date-time '{input}': expected RFC 3339 or local format 'YYYY-MM-DDTHH:MM[:SS]'")]
    InvalidDateTime { input: String },

    #[error("scheduled start time is in the past: {scheduled_at}")]
    PastDateTime { scheduled_at: String },

    #[error("local date-time '{input}' is ambiguous in the current timezone; provide an explicit UTC offset")]
    AmbiguousLocalDateTime { input: String },

    #[error("local date-time '{input}' does not exist in the current timezone; provide a valid time or explicit UTC offset")]
    NonexistentLocalDateTime { input: String },

    #[error("scheduled start time exceeds the supported date-time range")]
    DateTimeOverflow,

    #[error("failed to listen for cancellation signal: {0}")]
    Signal(#[source] std::io::Error),
}
```

If MACC has a shared CLI error model, map these cases into it rather than adding parallel top-level error handling.

---

## 12. Absolute time parsing algorithm

Use a deterministic parse order.

### 12.1 Algorithm

```text
Input string
   |
   +-- Parse as RFC 3339 with explicit offset
   |      |
   |      +-- Success: convert to local timezone and validate future time
   |
   +-- Parse as local YYYY-MM-DDTHH:MM:SS
   |      |
   |      +-- Resolve with Local.from_local_datetime(...)
   |
   +-- Parse as local YYYY-MM-DDTHH:MM
          |
          +-- Resolve with Local.from_local_datetime(...)
```

### 12.2 Conceptual Rust code

```rust
use chrono::{DateTime, Local, LocalResult, NaiveDateTime, TimeZone};

fn parse_absolute_time(
    input: &str,
    now: DateTime<Local>,
) -> Result<DateTime<Local>, DelayedRunError> {
    if let Ok(value) = DateTime::parse_from_rfc3339(input) {
        let local = value.with_timezone(&Local);
        return validate_future_time(local, now);
    }

    let naive = ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(input, format).ok())
        .ok_or_else(|| DelayedRunError::InvalidDateTime {
            input: input.to_owned(),
        })?;

    let local = match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => value,
        LocalResult::Ambiguous(_, _) => {
            return Err(DelayedRunError::AmbiguousLocalDateTime {
                input: input.to_owned(),
            });
        }
        LocalResult::None => {
            return Err(DelayedRunError::NonexistentLocalDateTime {
                input: input.to_owned(),
            });
        }
    };

    validate_future_time(local, now)
}
```

This snippet is illustrative. Adapt error handling and imports to MACC conventions.

---

## 13. Dependency policy

Before adding dependencies, inspect the workspace for existing equivalents.

Preferred choices:

| Need | Preferred library | Rule |
|---|---|---|
| Human duration parsing | `humantime` | Reuse an existing parser if already present |
| Date-time parsing and local timezone | `chrono` | Reuse existing workspace version |
| Async sleep and Ctrl+C | `tokio` | Already aligned with MACC architecture |
| Error derivation | `thiserror` | Reuse existing error pattern |
| CLI arguments | `clap` | Existing MACC CLI framework |

Dependencies should be declared at workspace level if that is the repository convention.

Do not introduce:

- a cron parser;
- a scheduler library;
- a daemon framework;
- a persistent job queue;
- a database migration;
- an OS-specific scheduling crate.

---

## 14. Integration boundaries

## 14.1 Core coordinator

No coordinator state-machine behavior should change.

The feature must not alter:

- task selection;
- state transitions;
- performer dispatch;
- worktree reuse;
- merge handling;
- retry policy;
- rate-limit backoff;
- tool fallback;
- quota pause behavior;
- PRD synchronization;
- cleanup behavior.

## 14.2 Canonical configuration

No new `.macc/macc.yaml` property is required.

Do not add fields such as:

```yaml
automation:
  coordinator:
    schedule: ...
```

The delay is an ephemeral CLI request, not project configuration.

## 14.3 Runtime state and logs

No persistent delayed-run state file is required.

Do not create:

```text
.macc/state/scheduler.json
.macc/automation/schedules.json
.macc/log/scheduler/
```

Once the coordinator starts, use normal coordinator logs and state.

Before it starts, standard CLI output is sufficient.

## 14.4 TUI and Web Client

No new screen, route, endpoint, or UI control is required in this version.

The command may still be launched from an embedded terminal, but the delayed-run feature itself is CLI-only.

## 14.5 Wrapper scripts

If `coordinator.sh` forwards arguments to the native coordinator, verify that it preserves:

```text
--in
--at
```

No scheduling behavior should be implemented in shell scripts.

---

## 15. Security and safety considerations

## 15.1 No early resource ownership

The delayed process must not reserve coordinator resources while waiting. This prevents a future run from blocking current legitimate work.

## 15.2 No implicit past-time execution

Rejecting past timestamps prevents accidental immediate autonomous execution.

## 15.3 No detached execution

The initial version must remain attached to the invoking terminal. This makes process ownership and cancellation obvious.

## 15.4 Working directory

Resolve and retain the intended project working directory when the command starts.

Recommended behavior:

- Convert the project path to an absolute/canonical path before waiting.
- At execution time, verify that it still exists.
- Do not silently switch to a different project based on a later shell working-directory change.

A running process normally retains its own current directory, but an explicit resolved project path makes behavior easier to test and reason about.

## 15.5 Changed or deleted project

If the project is deleted, moved, or becomes inaccessible during the wait, fail at start time with the normal project-resolution error.

Do not attempt to discover a replacement project.

## 15.6 Credentials and environment

The delayed run inherits the process environment captured at command launch.

This means credentials or environment variables changed in another shell during the wait are not automatically imported. This is standard process behavior and does not require additional machinery.

---

## 16. Test strategy

Testing must avoid real long waits.

## 16.1 Unit tests: duration parsing

Required cases:

| Test | Expected result |
|---|---|
| `30s` | 30 seconds |
| `30m` | 30 minutes |
| `2h` | 2 hours |
| `1h 30m` | 90 minutes, if supported by selected parser |
| `0s` | `ZeroDuration` |
| empty input | invalid duration |
| `abc` | invalid duration |
| negative input | invalid duration |
| overflow input | controlled error |

## 16.2 Unit tests: absolute parsing

Required cases:

| Test | Expected result |
|---|---|
| RFC 3339 with positive offset | Correct instant |
| RFC 3339 with `Z` | Correct instant |
| Local `YYYY-MM-DDTHH:MM` | Correct local instant |
| Local `YYYY-MM-DDTHH:MM:SS` | Correct local instant |
| Invalid separators | Invalid date-time |
| Invalid calendar date | Invalid date-time |
| Past value | Past date-time error |
| Ambiguous local DST time | Ambiguous local date-time error where test environment supports it |
| Nonexistent local DST time | Nonexistent local date-time error where test environment supports it |

Timezone-sensitive tests should not rely on the developer machine's local timezone. Prefer:

- extracting timezone-resolution logic so a deterministic timezone can be injected in tests; or
- limiting platform-local tests and testing ambiguous/nonexistent handling through a timezone-aware helper.

Do not mutate the global process timezone in tests that run in parallel unless the test framework serializes them.

## 16.3 Unit tests: schedule resolution

Required cases:

- neither flag returns `None`;
- only `--in` returns a relative schedule;
- only `--at` returns an absolute schedule;
- conflicting values are rejected by Clap;
- target delay is calculated correctly from injected `now`;
- relative target overflow is handled.

## 16.4 Async tests: waiting

Use Tokio's paused-time testing support where available:

```rust
#[tokio::test(start_paused = true)]
async fn waits_until_the_delay_expires() {
    // Arrange schedule.
    // Start wait future.
    // Advance virtual time.
    // Assert Ready.
}
```

The production `Ctrl+C` listener is difficult to trigger safely in a unit test. Improve testability by allowing the wait primitive to accept an abstract cancellation future internally:

```rust
async fn wait_with_cancellation<C>(
    delay: Duration,
    cancellation: C,
) -> Result<DelayedRunOutcome, DelayedRunError>
where
    C: Future<Output = Result<(), std::io::Error>>,
```

Production code passes `tokio::signal::ctrl_c()`. Tests pass a controlled future.

Required cases:

- delay completes first -> `Ready`;
- cancellation completes first -> `Cancelled`;
- cancellation listener fails -> signal error;
- cancellation path does not invoke coordinator execution.

## 16.5 CLI parsing tests

Required commands:

```bash
macc coordinator run
macc coordinator run --in 30m
macc coordinator run --at 2026-06-28T02:00
```

Required failures:

```bash
macc coordinator run --in 30m --at 2026-06-28T02:00
macc coordinator status --in 30m
macc coordinator cleanup --at 2026-06-28T02:00
```

Assertions should verify:

- correct subcommand ownership;
- correct field mapping;
- conflict messaging;
- help output includes examples or format guidance.

## 16.6 Integration tests

Use very short durations, such as 50–200 milliseconds, only through an internal test hook or direct function invocation. User-facing duration syntax does not need to advertise millisecond scheduling unless the selected parser already supports it.

Required integration scenarios:

1. Immediate run invokes the existing run path without delay.
2. Relative delayed run invokes the existing run path once.
3. Absolute delayed run invokes the existing run path once.
4. Cancelled delayed run invokes the existing run path zero times.
5. Project config changed during wait is read at actual start.
6. Project removed during wait produces a normal start-time error.
7. Another coordinator active at start produces the normal lock/conflict error.
8. Existing coordinator CLI overrides survive the delay.

Use a mock or test seam around coordinator execution rather than launching real AI performers.

## 16.7 Regression tests

Verify that these existing commands are unaffected:

```bash
macc coordinator
macc coordinator run
macc coordinator status
macc coordinator dispatch
macc coordinator advance
macc coordinator reconcile
macc coordinator cleanup
```

Also verify that coordinator help remains understandable and that scheduling flags do not leak into unrelated actions.

---

## 17. Documentation changes

## 17.1 CLI help

Recommended help text:

```text
Start the coordinator immediately or after a one-shot delay.

Examples:
  macc coordinator run
  macc coordinator run --in 30m
  macc coordinator run --at "2026-06-28T02:00"
  macc coordinator run --at "2026-06-28T02:00:00+02:00"

Notes:
  --in and --at are mutually exclusive.
  A date-time without an explicit offset uses the machine's local timezone.
  The process must remain running until the scheduled start time.
  Press Ctrl+C to cancel before execution begins.
```

## 17.2 Main MACC specification

Update the coordinator command section with:

```markdown
### Delayed one-shot coordinator run

`macc coordinator run` supports an optional process-bound delayed start:

```bash
macc coordinator run --in 30m
macc coordinator run --at "2026-06-28T02:00"
```

- `--in` accepts a human-readable relative duration.
- `--at` accepts RFC 3339 or local `YYYY-MM-DDTHH:MM[:SS]` date-time.
- A local date-time without an offset uses the machine's local timezone.
- The flags are mutually exclusive.
- Past timestamps and zero durations are rejected.
- The process must remain alive until execution.
- `Ctrl+C` cancels the pending run.
- No scheduler state is persisted.
```

## 17.3 README or user guide

Add a short operational example and explicitly state the limitations:

- terminal must remain open;
- no restart persistence;
- no recurring schedule;
- no background service;
- no OS scheduler integration.

## 17.4 Changelog

Recommended entry:

```markdown
### Added

- Added delayed one-shot coordinator execution with `macc coordinator run --in <duration>` and `macc coordinator run --at <datetime>`.
```

---

## 18. Implementation work breakdown

## Task 1 — Confirm existing coordinator run entry point

**Goal:** Identify the single function currently used for `macc coordinator` and `macc coordinator run`.

Actions:

1. Locate the Clap coordinator command definitions.
2. Locate the native Rust `run` handler.
3. Confirm how default `macc coordinator` maps to `run`.
4. Identify the current coordinator lock/preflight path.
5. Identify output and error abstractions.

Deliverable:

- A documented function boundary that delayed execution will call after waiting.

Constraint:

- Do not refactor coordinator internals unless required to expose a reusable run function.

## Task 2 — Add CLI arguments

**Goal:** Add `--in` and `--at` only to the `run` action.

Actions:

1. Add `run_in: Option<String>` exposed as `--in`.
2. Add `at: Option<String>` exposed as `--at`.
3. Add mutual exclusion through Clap.
4. Add value names and help text.
5. Add parsing tests.

Deliverable:

- Valid CLI syntax with clear help and conflict errors.

## Task 3 — Implement pure schedule resolution

**Goal:** Parse and validate relative and absolute start values.

Actions:

1. Parse human-readable durations.
2. Reject zero duration.
3. Parse RFC 3339 timestamps.
4. Parse local timestamps with and without seconds.
5. Handle ambiguous and nonexistent local times.
6. Reject past times.
7. Calculate the wait duration safely.
8. Add deterministic unit tests.

Deliverable:

- Pure, independently tested resolution functions.

## Task 4 — Implement cancellation-safe waiting

**Goal:** Wait without blocking the async runtime and allow `Ctrl+C` cancellation.

Actions:

1. Use `tokio::time::sleep`.
2. Use `tokio::select!` with `tokio::signal::ctrl_c()`.
3. Return a typed `Ready` or `Cancelled` outcome.
4. Avoid coordinator mutations during the wait.
5. Add virtual-time tests and a cancellation test seam.

Deliverable:

- Async waiting logic with no polling loop.

## Task 5 — Integrate with coordinator execution

**Goal:** Handoff to the existing coordinator run path.

Actions:

1. Resolve schedule before coordinator execution.
2. Print schedule summary unless quiet mode suppresses it.
3. Wait or return cleanly when cancelled.
4. Print start transition.
5. Invoke the same coordinator run function as an immediate run.
6. Confirm that config and preflight checks occur at actual start.
7. Confirm no coordinator lock is held during the delay.

Deliverable:

- One-shot delayed coordinator execution with no duplicated run logic.

## Task 6 — Validate conflicts and state changes

**Goal:** Verify correct behavior when the project changes during the wait.

Actions:

1. Test another coordinator starting before the target time.
2. Test the project being removed or made invalid.
3. Test configuration updates during the wait.
4. Test PRD updates during the wait.
5. Confirm CLI overrides remain retained.

Deliverable:

- Integration coverage for realistic delayed-run races.

## Task 7 — Update documentation

**Goal:** Make the feature discoverable without implying persistent scheduling.

Actions:

1. Update command help.
2. Update the MACC coordinator specification.
3. Update README/user guide.
4. Add changelog entry.
5. Include limitations explicitly.

Deliverable:

- User-facing documentation aligned with actual behavior.

---

## 19. Definition of done

The feature is complete when all of the following are true:

### CLI

- [ ] `macc coordinator run` still starts immediately.
- [ ] `macc coordinator run --in 30m` is accepted.
- [ ] `macc coordinator run --at "2026-06-28T02:00"` is accepted.
- [ ] RFC 3339 values with offsets are accepted.
- [ ] `--in` and `--at` cannot be combined.
- [ ] Scheduling flags are unavailable on non-`run` coordinator actions.

### Validation

- [ ] Invalid durations return a clear error.
- [ ] Zero duration is rejected.
- [ ] Invalid date-times return a clear error.
- [ ] Past date-times are rejected.
- [ ] Ambiguous local times are rejected.
- [ ] Nonexistent local times are rejected.
- [ ] Date-time overflow is handled without panic.

### Runtime

- [ ] Relative delay uses a non-blocking Tokio timer.
- [ ] The normal coordinator run path is called after the delay.
- [ ] No coordinator mutation occurs before the target time.
- [ ] No coordinator lock is held throughout the waiting period.
- [ ] Project/config preconditions are evaluated at actual start.
- [ ] Another active coordinator is handled by the existing conflict mechanism.
- [ ] Existing coordinator flags work unchanged with delayed execution.

### Cancellation

- [ ] `Ctrl+C` before start cancels the delayed run.
- [ ] Cancellation exits cleanly without starting the coordinator.
- [ ] Cancellation does not create task, worktree, registry, or scheduler state.
- [ ] Signal behavior after coordinator start is unchanged.

### Quality

- [ ] Pure parsing and resolution logic has unit tests.
- [ ] Waiting logic has async tests without real long delays.
- [ ] CLI parsing and conflict behavior has tests.
- [ ] Integration tests verify single invocation and cancellation.
- [ ] Existing coordinator commands pass regression tests.
- [ ] Code comments and documentation are in English.
- [ ] Formatting, linting, build, and test suites pass.

### Documentation

- [ ] CLI help includes examples.
- [ ] Main MACC documentation describes the feature.
- [ ] Limitations are explicit.
- [ ] Changelog is updated.

---

## 20. Acceptance scenarios

## Scenario 1 — Immediate execution remains unchanged

**Given** a valid MACC project  
**When** the user runs:

```bash
macc coordinator run
```

**Then** the coordinator starts immediately through the existing run path.

## Scenario 2 — Relative delayed execution

**Given** a valid MACC project  
**When** the user runs:

```bash
macc coordinator run --in 30m
```

**Then** MACC displays the target time, waits without starting coordinator work, and starts the normal coordinator run after approximately 30 minutes.

## Scenario 3 — Absolute local execution

**Given** a future local machine date-time  
**When** the user runs:

```bash
macc coordinator run --at "2026-06-28T02:00"
```

**Then** MACC interprets the value in the machine's local timezone and starts at the resolved instant.

## Scenario 4 — Absolute offset-aware execution

**Given** a future RFC 3339 timestamp  
**When** the user runs:

```bash
macc coordinator run --at "2026-06-28T02:00:00+02:00"
```

**Then** MACC starts at that exact instant regardless of the machine's display timezone.

## Scenario 5 — Cancellation

**Given** a delayed coordinator run is waiting  
**When** the user presses `Ctrl+C`  
**Then** MACC prints a cancellation message, exits successfully, and does not start the coordinator.

## Scenario 6 — Conflicting arguments

**When** the user runs:

```bash
macc coordinator run --in 30m --at "2026-06-28T02:00"
```

**Then** argument parsing fails before execution and explains that the options are mutually exclusive.

## Scenario 7 — Past target

**Given** the target timestamp is in the past  
**When** the user supplies it through `--at`  
**Then** MACC returns an error and does not start immediately.

## Scenario 8 — Another coordinator becomes active

**Given** a delayed command is waiting  
**And** another coordinator starts before its target time  
**When** the delayed command reaches its target  
**Then** it invokes the normal run preflight and receives the existing active-coordinator conflict error.

## Scenario 9 — Config changes during the delay

**Given** a delayed command is waiting  
**And** `.macc/macc.yaml` is validly changed before the target time  
**When** the run begins  
**Then** the existing coordinator path loads the updated configuration while preserving explicit CLI overrides.

## Scenario 10 — Project disappears during the delay

**Given** a delayed command is waiting  
**And** the project path is removed or becomes inaccessible  
**When** the target time is reached  
**Then** the command fails with the normal project/preflight error and does not attempt recovery through another directory.

---

## 21. Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Duplicate coordinator logic | Divergent behavior and bugs | Handoff to the existing run function only |
| Lock held during wait | Blocks legitimate coordinator runs | Acquire/check execution lock only at actual start |
| Past timestamp runs immediately | Unexpected autonomous work | Reject all past targets |
| DST ambiguity | Wrong start time | Reject ambiguous/nonexistent local time and request explicit offset |
| Terminal closes | Run never starts | Document process-bound limitation clearly |
| Project changes while waiting | Stale assumptions | Load config and execute preflight at actual start |
| Real-time tests are slow/flaky | Poor test suite | Inject time and use Tokio paused-time tests |
| Added dependency overlap | Unnecessary maintenance | Reuse existing workspace crates first |
| Unstructured output breaks JSON mode | Automation regression | Use existing output abstraction where applicable |
| Scope expands into scheduler | Excess complexity | Enforce non-goals and avoid persistence/UI/recurrence |

---

## 22. Recommended final design decision record

### Decision

Implement delayed one-shot execution directly on `macc coordinator run` using:

```bash
macc coordinator run --in <DURATION>
macc coordinator run --at <DATETIME>
```

### Rationale

- It solves an immediate operational need.
- It aligns with the existing native coordinator command.
- It requires no service, daemon, persistent job store, or OS integration.
- It preserves a clear lifecycle: the invoking process owns the pending run.
- It is easy to cancel and easy to remove or evolve later.

### Consequences

Positive:

- Small implementation surface.
- Low operational risk.
- Cross-platform with existing Rust/Tokio architecture.
- No configuration migration.
- No scheduler maintenance burden.

Accepted limitations:

- The process must remain alive.
- The terminal must remain open unless the user independently manages the process.
- The delayed run does not survive reboot or process termination.
- There is no recurrence or persistence.
- Wall-clock changes after an `--at` value is resolved do not recalculate the wait.

### Future evolution

No follow-up scheduling architecture is required now. Persistent or recurring scheduling should only be reconsidered if real usage demonstrates a need that cannot be met by this one-shot mechanism.

---

## 23. Suggested implementation order

Implement in this sequence to minimize regression risk:

1. Identify and isolate the existing coordinator run entry point.
2. Add CLI arguments and Clap conflict rules.
3. Implement pure duration/date-time parsing.
4. Add parsing tests.
5. Implement async wait and cancellation.
6. Integrate handoff to the existing run function.
7. Add integration and regression tests.
8. Update command help and MACC documentation.
9. Run formatting, lint, build, and full test suites.

The feature should be delivered as one focused change set without unrelated coordinator refactoring.

---

## 24. Final implementation summary

The intended implementation is deliberately simple:

```text
optional CLI delay
      -> cancellable async wait
      -> existing coordinator run
```

No scheduler is created. No schedule is persisted. No recurring behavior is introduced. The feature merely postpones the start of the existing coordinator command while preserving all of its current execution, safety, observability, and recovery behavior.
