# PRD queue transitions

`macc coordinator sync` and the native coordinator run share a queue transition service. CLI, TUI and Web launches use the same preview and runtime guards. No provider is invoked to manage the queue.

| Change | Behavior |
| --- | --- |
| Queue unchanged | Keep the cursor; a completed queue does not restart itself. |
| Strict append, old entries unchanged | Keep the cursor and previous delivery evidence. Resume the first new PRD after the old series finishes. |
| Replacement/reorder after completed delivery | Verify saved completed registries and gates, archive the old generation, and start at the first proposed PRD. |
| Replacement during an incomplete series | Require explicit review/acceptance. Never discard outstanding work. |
| Modified/reused delivered task identity | Reject reuse of a task ID from another recorded contract. Keep the original PRD contract or assign a fresh task ID. A reset cannot bypass this guard; an undelivered task may retain its ID after explicit review. |
| Live coordinator, claimed worker, active supervisor repair, dirty/unmerged worktree | Reject the transition until that work is integrated or explicitly resolved. |

Automatic replacement requires `completed` progress, a cursor at the end, all per-PRD completion snapshots satisfying the existing delivery/gate rules, and no unfinished current-registry tasks. Missing legacy evidence requires review rather than inferred completion. Append-only changes to a completed series use the same delivery check. The original PRD source files may have been removed; retained completion evidence remains authoritative for this check.

Read-only launch/Doctor previews select the proposed task source without accepting or changing queue progress. The native command rechecks under `.macc/state/prd-queue.lock` before importing tasks. An explicit `--prd` selection retains its existing separate behavior.

## Review and acceptance

```sh
macc coordinator prds check
macc coordinator prds status
# Only for an intended change that needs explicit acceptance:
macc coordinator prds reset
# Non-interactive acceptance:
macc coordinator prds reset --confirm
macc coordinator sync
```

CLI reset explains the action and requires confirmation. The TUI queue editor retains its reset confirmation. In the Web launch dialog, **Review PRD queue change…** shows additions/removals/modifications and the old completion count. Acceptance requires a checkbox and pins the accepted PRD fingerprints; its revision must still match the saved progress, configuration and current PRD fingerprints. It resets progress without launching work, then refreshes launch conditions. Ownership checks and runtime/workspace guards still apply. Unfinished tasks outside the next PRD prevent registry replacement even after explicit acceptance.

API: `GET /api/v1/coordinator/prd-queue`; `POST /api/v1/coordinator/prd-queue/accept` with `{ "revision": "...", "confirmed": true }`. The POST requires the existing project owner gate.

Queue acceptance does not overwrite durable decision plans or waive decision approvals. For an existing PRD managed by decision control, use the existing native graph-change/reconciliation workflow for contract edits. New series should use new PRD files and fresh task IDs for new contracts.

## History and diagnostics

Before automatic transitions and explicit resets, MACC writes a UUID generation under `.macc/state/prd-queue-history/`: previous progress, current registry, available per-PRD completion/transition registries and transition metadata. Completed-index files can then be reused by the next series without overwriting the archived proofs. Existing external delivery dependencies are preserved. Referenced commit identities and worktree metadata remain in the snapshots; this operation never resets files, deletes branches or performs a merge.

`MACC-PRD-QUEUE-TRANSITION` identifies changes needing attention. The CLI prints its actionable error once. The supervisor treats it as queue/configuration review, without paid AI diagnosis or an application source repair; queue/configuration changes update its waiting evidence.

No database schema migration is needed. Existing progress files remain readable. Existing projects with incomplete historical completion snapshots may need explicit review/reset once. This does not waive delivery gates or authorize destruction of retained work.
