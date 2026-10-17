# O2 P3-D — Reviewed adaptive policy tuning

## Delivered behavior

P3-D adds a local, supervised policy tuning loop: pre-register one candidate, run
controlled paired tasks, seal quality evidence, compare actual incremental costs,
and explicitly adopt a passing recommendation. CLI, TUI and Web share the same
Rust application operations. There is no provider-specific dispatch, optimizer
worker, package installation, paid call or automatic configuration write.

Trials are disabled by default. Existing session isolation, PRD restoration,
review, memory promotion and source-freshness rules remain mandatory. No SQLite
upgrade is introduced: plans, invocation provenance, sealed observations, reports
and adoption receipts reuse canonical `o2_audit` and are included in SQLite
snapshots saved by `macc save`.

Release and acceptance descriptions should call this **reviewed, supervised
policy tuning**. Operators select candidates, control representative independent
tasks, keep contracts identical and attest quality. Shipping this workflow does
not demonstrate an autonomous optimizer or an achieved project saving. Projects
whose adapters cannot report exact monetary cost cannot adopt a trial through
this gate; estimated token pricing and subscription quotas do not replace it.

## Contracts and boundaries

`core/src/o2/tuning/` separates typed contracts, pre-registration/inspection,
invocation provenance, measured observations, statistics and reviewed adoption.
It consumes the existing tool-neutral `session_store::usage` normalization port.
Native provider parsers, usage scopes and currencies remain in ToolSpec adapters.
No new agent-callable MCP tool or privilege is introduced.

A plan records the complete baseline O2 policy, one allowed numeric change, the
fixed task roster, each task's full starting Git revision, an inspected protocol
file/checksum, and frozen evaluation thresholds. One trial may be open at a time.
The safe field list is returned by `tuning_status` and includes memory ranking
weights, optional context budgets and existing compaction thresholds/minimum turns.
Flags, scope/security constraints, provider executables and installation controls
cannot be optimized through this workflow. Ordinary canonical policy validation
still applies, including ordered compaction thresholds and integer ranges.

Registration does not select an arm, dispatch a task or change any policy.
Operators use the existing revision-checked policy preview/apply operation to
select exactly the registered baseline or candidate between controlled runs.
Restore the complete baseline before previewing final adoption. A semantic-weight
trial does not grant P3-C semantic approval: satisfy its separate exact-settings
benchmark gate for each arm when comparing hybrid retrieval. Provider deployment
and native-setting adoption gates likewise remain separate.

## Designing and running an experiment

1. Enable `context_engine.tuning_enabled` through the shared policy editor.
   Ensure invocation usage capture is enabled/configured through the existing
   session/usage policies and the selected adapter reports exact incremental
   monetary cost, currency and model. Subscription quotas, token estimates and
   missing charges are not exact monetary evidence.
2. Write a safe repository protocol file specifying representative independent
   tasks, objective PRD/test checks, review severity scale, controlled tool/model/
   effort, repository/dependency state, memory/provider corpus, cache/environment
   controls and treatment of all failures, retries and interruptions. Do not
   select tasks after observing their cost advantage. Randomize representative
   task order externally; the registered roster is immutable.
3. Register at least the configured minimum number of distinct task IDs (20 by
   default, at most 100). Registration fixes the sample; ties may require a larger
   initial roster because at least the configured minimum **non-tied** pairs must
   remain informative. Each pair represents the same task contract, executed from the same clean
   starting Git revision under two unused identities: `<pair-id>--baseline` and
   `<pair-id>--candidate`. Register these as isolated copies of the original task
   contract, with identical PRD/acceptance checks. MACC refuses identities with
   pre-existing sessions/checkpoints/context; each arm keeps its own checkpoints
   and starts in separate fresh native/logical sessions. Preserve
   output branches/evidence externally before preparing the other run. MACC does
   not reset or clean working trees for this experiment.
4. Case 1 runs baseline then candidate, case 2 candidate then baseline, alternating
   thereafter. Use idle isolated worktrees and the exact recorded policies.
   Include every invocation for each arm, including failure/correction calls.
   Do not rerun an unfavorable arm and substitute the cheaper attempt.
5. After both arms finish, inspect their result artifacts and seal a paired quality
   observation with a safe evidence file path and its SHA-256. There is one sealed
   observation per task; it cannot be edited to discard an unfavorable result.
   Operator attestation covers acceptance, tests, specification fidelity, maximum
   review finding severity (0 none through 4 critical), and retries for each arm.
   MACC verifies the file's identity and telemetry provenance; it does not infer
   task acceptance from a tool exit code or claim that a file hash proves quality.
6. Stop the coordinator/supervisor and finish/recover outstanding leases, then
   evaluate the complete fixed sample. Incomplete or unmeasurable trials may be
   explicitly cancelled; cancellation keeps the plan and statistical allocation.
7. Inspect the immutable report, restore baseline policy, preview the recommendation
   and confirm the exact report checksum/configuration revision. Adoption requires
   stopped processes and no leases. Changes affect subsequent invocations/queries;
   existing provider deployment remains separate.

### Provenance and measurement requirements

Usage begin/end participate in their existing canonical SQLite transactions.
For the derived execution task identities in an open trial they record policy fingerprints, starting
Git revision/clean state, execution configuration fingerprint and the approved
memory/provider corpus fingerprint. The policy must match a registered arm at
invocation start and end. Changing approved memories/provider receipts during a
paired task invalidates its comparison. Missing receipts and unstamped calls in
a trial session also invalidate the comparison. The entire execution-task usage
ledger is counted, so an unstamped whole session cannot disappear from costs.
Disabling the feature stops new
trial stamps, making incomplete evidence ineligible rather than fabricating it.

Both arms must use matching tool/model/role/phase/schema/version/configuration
profiles and currencies; all invocations must have normalized exact incremental
cost, model and elapsed time. Cumulative provider totals are normalized upstream
and never added directly here. Version comparison uses reported values; an unknown version does not prove the
installed binaries match. Pin installed tools/dependencies in the protocol.
Costs retain the existing integer nano-currency
units; only ratios are compared. A zero/unknown baseline cost cannot establish a
cost-saving claim. Reports include per-task totals and a checksum over canonical
usage records and the quality observation. Final preview/apply revalidates these
records and evidence files, so changing telemetry after evaluation rejects adoption.

At most 1,000 invocation stamps per trial are supported; exceeding this explicit
bound makes evaluation ineligible. Both arms must retain all canonical usage
records and separate native sessions.

The latency measure is **summed invocation elapsed time**, with the precision of
existing usage telemetry. It excludes queue waits, manual review delays and other
wall-clock gaps. It is not end-to-end completion latency or an invoice guarantee.
Git cleanliness covers normal Git status, not ignored dependency/build files.
The protocol must control those inputs, independent task sampling, environment,
cache effects and any mutations between invocation boundaries. Policy stamps
verify invocation boundaries; they do not monitor every instant of a running call.

## Evaluation gate

The single pre-registered primary hypothesis is lower paired task cost. The exact
one-sided sign test computes `P[Binomial(wins + losses, 0.5) >= wins]`; equal-cost
pairs are excluded from that test but stay in the report and aggregate totals.
This follows the paired sign-test definition in the
[NIST statistical reference](https://www.itl.nist.gov/div898/software/dataplot/refman1/auxillar/signtest.htm).
Its interpretation requires independent, representative paired differences;
distinct task IDs alone cannot prove those experimental assumptions.

A passing report additionally requires:

- Every registered task has a sealed paired outcome and complete comparable usage.
- At least the configured minimum non-tied task pairs.
- A p-value no greater than the frozen allocated alpha.
- Aggregate cost reduction at least the configured effect size.
- Aggregate summed invocation latency no greater than the configured regression.
- Every candidate task accepted, tests passing and faithful to its specification;
  no candidate increase in severity or retries relative to its paired baseline.
- A single comparable currency throughout the sample.

To limit repeated-trial false positives in a project, trial ordinal `i` receives
`min(configured_alpha, 0.05 / (i * (i + 1)))`. Cancelled trials consume an ordinal.
The allocation sums to at most 0.05 across project trials. No early success or
sample replacement is supported. A project-wide recommendation does not guarantee the same benefit for tools,
models or task classes absent from the trial protocol. A failed trial remains inspectable and cannot
be adopted through the tuning operation. This is a cost-advantage test plus
quality guardrails, not statistical proof of equal quality on every future task,
a causal experiment certification or a universal performance guarantee.

## Shared configuration

All fields are editable in CLI policy actions, TUI **5 Policies**, and Web
**Context configuration** or **Policies**. Bounds/defaults come from the Rust
catalog; saves use the existing preview/apply revision contract.

| Canonical field | Default | Bounds | Effect |
|---|---:|---|---|
| `context_engine.tuning_enabled` | `false` | Boolean | Permit new registration and invocation stamps |
| `context_engine.tuning_min_pairs` | `20` | 20–100 | Required non-tied independent task pairs |
| `context_engine.tuning_alpha` | `0.01` | 0.001–0.05 | Maximum allocated one-sided trial alpha |
| `context_engine.tuning_min_cost_reduction` | `0.05` | 0.01–0.9 | Minimum aggregate measured cost reduction |
| `context_engine.tuning_max_latency_regression` | `0.10` | 0–0.5 | Maximum relative invocation-latency increase |

Thresholds are frozen at registration. Changing them during a trial does not
retroactively relax the gate and makes invocation policy differ from its arms.
Safety field allowlisting, exact-cost requirements and separate confirmation are
mandatory invariants, not configurable bypasses.

## CLI

```bash
macc o2 tuning status
macc o2 tuning create --confirm < reviewed-trial-plan.json
macc o2 tuning status TRIAL_ID
macc o2 tuning observe TRIAL_ID --confirm < reviewed-task-outcome.json
macc o2 tuning close TRIAL_ID --confirm
# Or explicitly cancel an incomplete trial:
macc o2 tuning close TRIAL_ID --cancel --confirm
macc o2 tuning preview REPORT_ID REPORT_CHECKSUM
macc o2 tuning apply REPORT_ID REPORT_CHECKSUM CONFIG_REVISION --confirm
```

Example plan shape (supply the full pre-registered roster of at least 20):

```json
{
  "field": "context_engine.push_memory_limit_tokens",
  "candidate": 800,
  "cases": [{"task_id": "TASK-001", "revision": "FULL_GIT_COMMIT_HASH"}],
  "protocol_path": "experiments/context-policy-protocol.md",
  "protocol_sha256": "SHA256_OF_INSPECTED_PROTOCOL"
}
```

The plan `task_id` is a pair key. Run its two task copies as `TASK-001--baseline`
and `TASK-001--candidate`; `macc o2 tuning status TRIAL_ID` lists this mapping. Keep their
PRD/checks identical and control ignored environment files in the protocol.

Example quality observation (use the pair key; costs/latency are intentionally absent):

```json
{
  "task_id": "TASK-001",
  "baseline": {"accepted": true, "tests_passed": true, "spec_fidelity": true,
               "max_finding_severity": 0, "retries": 0},
  "candidate": {"accepted": true, "tests_passed": true, "spec_fidelity": true,
                "max_finding_severity": 0, "retries": 0},
  "evidence_path": "experiments/results/task-001.md",
  "evidence_sha256": "SHA256_OF_INSPECTED_RESULTS"
}
```

The six corresponding typed `macc o2 action` operations are `tuning_status`,
`tuning_create`, `tuning_observe`, `tuning_close`, `tuning_preview` and `tuning_apply`.
Mutation actions require `confirmed: true`; apply also requires the preview's
configuration revision. Agent sessions cannot perform privileged trial mutations.

## Interactive clients

- **TUI → O2 → 7 Policy trials:** Left/Right selects the safe field; `n` guides
  registration, `i` inspects trials, `q` collects typed quality/evidence, `e` prepares
  evaluation, `x` prepares cancellation, `p` previews a passing report, `s` prepares
  adoption. Every mutation requires the separate `a` confirmation. Invocation and
  evaluation jobs run off the terminal event loop. Navigation invalidates prepared
  tuning mutations; adoption is bound to the exact inspected preview.
- **Web → O2 → Policy trials** (`/ops/tuning`, `/config/tuning`): inspect/select
  trials, choose a safe field, enter the fixed task roster/revision and evidence,
  use checkboxes/severity choices for paired quality, inspect reports and preview
  adoption. Prepare, inspect and explicitly confirm each mutation. Changing inputs
  invalidates the prepared operation. Trial identity fences reject late responses.
  Configuration is available in the same page in configuration mode.

The guided registration form uses a common starting commit for its task roster.
The typed CLI/action contract also supports per-task starting commits. File paths
and checksums are deliberately explicit: clients cannot fabricate evidence or
silently run arbitrary project test commands.

## Receipt integrity

The existing pinned `serde_json` dependency enables `float_roundtrip` so serialized
`f64` measurements and policy thresholds retain their bits after JSON/SQLite
reloading. Otherwise a last-bit parsing difference could change an exact report
checksum. This is not arbitrary-precision arithmetic; monetary totals remain
normalized integer nano-currency. No package/version or SQLite migration is added.

## Persistence, failure recovery and rollback

Plans/observations/reports are append-only audit events. Inspect a trial for its
provenance, observations, final report and adoption intent/completion records.
Configuration YAML and SQLite are different authorities: final adoption durably
records an intent before the canonical configuration CAS, then records completion.
A failure between them is visible as an incomplete intent. Inspect the current
policy/revision and before/after values before taking further action; do not
blindly replay an interrupted adoption. A revision conflict preserves existing
settings and requires a new reviewed preview. Unrelated canonical fields are
preserved by the shared configuration transaction.

Rollback uses the normal reviewed policy editor to restore baseline. Evidence
files must remain available with their inspected bytes through final adoption;
version them with the experiment artifacts. These files, the audit and reports
are not promoted memories. Legacy SQLite snapshots need no P3-D schema migration;
older binaries do not understand the new strict policy fields, so restore a
compatible configuration snapshot when downgrading.

Validation uses synthetic adapter telemetry and deterministic task fixtures. No
production trial or billed saving is claimed by shipping the implementation.
