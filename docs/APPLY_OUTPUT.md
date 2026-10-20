# Apply output and degraded preparation

## Normal use

```sh
macc apply                         # Compact review, explicit consent and preparation
macc apply --dry-run               # Compact preview; no project mutation
macc apply --verbose               # Detailed operations and technical command output
macc apply --quiet                 # Hide routine output; keep warnings and questions
macc apply --dry-run --json        # One JSON preview on stdout
macc apply --yes --json            # Explicit initial consent; one JSON execution result
```

The default review groups configuration changes, files already current, selected
tools, provider origins, migration/backup requirements and potential network work.
Significant detected security findings appear before consent. Normal execution
shows phase changes on stderr; diagnostic subprocess output is hidden unless
verbose. Account setup instructions remain visible when the operator must act.
The final summary groups configuration outcomes, storage/preparation status,
actionable warnings and evidence/backup paths. It does not enumerate hundreds of
unchanged files. Verbose output and saved JSON retain individual outcomes.

The terminal renderer uses plain status labels. No color or cursor animation is
required; redirected output has no terminal controls. `NO_COLOR` also disables
tracing colors. JSON stdout contains data only; diagnostics, questions and progress
use stderr. `--quiet` never hides essential warnings or account setup instructions.

A JSON flag or redirected stdin does not grant consent. Without a terminal,
mutating Apply requires `--yes`. That flag only confirms the initial reviewed
plan: it does not approve user-scope writes, network permission, semantic evidence,
or partial-index decisions. Such questions stop non-interactive execution with a
JSON error/nonzero exit and retained evidence. Dry-run remains non-mutating.

## Partial native indexes

The default native build budget is now 15,000 ms. Explicit existing values,
including 5,000 ms, are preserved. The maximum remains 60,000 ms.

An available runtime is different from complete index coverage. Apply inspects
adapter-owned quality diagnostics after lifecycle preparation. Parse failures and
partial coverage appear as warnings in CLI, TUI, Web and saved results. Failed
source parsing is not presented as a successful symbol result.

When the time budget is reached and can be increased, the assistant proposes a
bounded setting change (three times the current budget, at most 60 seconds).
The operator can:

- confirm the displayed setting, then review Apply again to rebuild;
- keep partial coverage, then review Apply again with warnings;
- adjust settings or cancel while keeping completed work.

Both setting updates and refreshed Apply plans remain revision checked.
A larger budget does not repair invalid source syntax. Other limits, unsupported
languages and parser errors remain explicit constraints. Degraded indexes are
eligible for a bounded rebuild during an explicit Apply; a healthy complete index
is reused. Coordinator startup keeps its existing local source-refresh behavior
and does not silently change budgets or start an approval conversation.

TUI assistant commands are contextual. `[i]` confirms a displayed budget suggestion;
`[Enter]` can retain partial coverage before the next Apply review. `[v]` toggles
technical output on the Apply workspace. Account instructions remain visible.
Web shows warnings beside preparation/results, offers both coverage decisions and
collapses technical output; unchanged result rows are available with a checkbox.

## Security messages

A path containing `tokens`, `settings` or `config` is not evidence of a credential.
The old automatic “Missing placeholders” warning has been removed. Actual content
scanning and secret rejection remain enabled. Templates may still use placeholders;
their mere absence is not a security finding.

## Evidence and result contracts

Each preparation attempt has a UUID under `.macc/log/apply/<run-id>/`:

- `preparation.json`: current or terminal state, completed steps, warnings and questions;
- bounded private command logs: full technical output for that attempt;
- `result.json`: successful execution summary, complete outcomes, warnings and backup paths.

The live state pointer remains `.macc/state/apply-preparation.json`. Starting a
new phase clears its previous output pointer. Clients display account output
separately from routine diagnostics; the CLI prints appended output instead of
repeating the whole buffer.

Execution JSON has `schema_version: 1`, `status` (`completed`,
`completed_with_warnings` or `failed`), and diagnostics/evidence appropriate to the
result. Successful results contain configuration counts, individual outcomes,
warnings and backup paths. Preparation/migration entries are not counted as
changed configuration files. Successful degraded preparation exits zero with
warnings after any required operator decision. Blocking errors exit nonzero.
Optional preparation report fields and defaulted inputs preserve old report/input
deserialization. The `ready` preparation status remains compatible; warnings carry
the coverage distinction instead of silently redefining provider availability.
