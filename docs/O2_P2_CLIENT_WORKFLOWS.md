# O2 P2-D: consolidated task workflows

P2-D connects the delivered memory, context, checkpoint/session and provider
services in the two operator clients. The shared Rust `overview` application
action supplies the same curated read model to CLI, TUI and Web. It is outside
the agent MCP catalog and cannot mutate memory, tasks, leases or providers.

## Web

Open **O2 Overview** in navigation (`/ops/overview`). Its task table shows recorded
sessions/lease records, checkpoints, latest-per-role reconciliation requirements,
context bundles and proposals. Project memory statuses and recorded provider
receipts are displayed separately. Choose **Focus TASK** or enter an exact task ID.
Use **First task page** / **Next task page** for bounded keyset pagination.

The focused task offers links to its memory inbox, latest context evidence and
sessions/checkpoints. Context details show optional estimated use/limit and
measurement quality; mandatory PRD content remains separate. Provider receipt
links preselect the provider without running health checks or a lifecycle action.

Operational/configuration links retain task scope. Configuration pages show the
relevant domain's typed controls; previews still include the entire policy draft
and preserve unrelated values. **Policies** shows all fields. Existing guarded
preview/apply actions remain required for writes and provider operations.
Switching domains retains the in-memory policy draft; leaving O2 for another
page also uses its existing session-storage draft mechanism. A storage-disabled
browser cannot guarantee restoration after unmount/reload.

Task scope uses `?task=TASK`. Context and checkpoint detail links use a domain-local
`id`; changing domain clears it to prevent treating one record type as another.
Provider links use `?provider=ID`. Context and Memory inspectors clear displayed
evidence, fragments and review confirmation when task/detail or access scope
changes, including manual filter edits. Late search/detail/error responses are
ignored, even after switching away and back. A scope change between evidence
loading and a follow-up review write prevents that write from being submitted;
an already accepted backend action is not rolled back or automatically retried.
Proposal and raw-output drafts are retained separately from inspected evidence. These extend existing routes rather than adding
separate backend state stores. Memory/source searches still require their existing
explicit search action. The session screen applies exact task scope alongside its existing text filter;
**Clear session task scope** returns to all tasks. It opens the linked checkpoint through the existing checksummed inspection service.

The overview refreshes every five seconds while visible, avoids overlapping
requests and ignores obsolete responses after a scope change. Failed refreshes
retain the current scope's snapshot with a stale indicator and explicit retry.
A different scope never inherits another scope's snapshot. Table headings,
button/link labels, keyboard focus and live error/status announcements support
assistive technology. The table supplies the numeric budget equivalent; no chart
is required to understand a value.

## TUI

Open Sessions (**J**), then **o**, then **6 Overview**. Existing tabs **1–5** retain
their shortcuts. Overview keys:

| Key | Operation |
| --- | --- |
| `/` | Exact task filter; blank returns to all tasks |
| `[` / `]` | First / next task page |
| Up / Down / Enter | Select / inspect the curated task evidence |
| `m` | Open memory with selected task scope |
| `t` | Open context with selected task scope |
| `s` | Return to sessions and select the first retained session for this task, if present |
| `5` / `6` | Policies / return to the overview |
| `r` | Refresh |

The Sessions **h** checkpoint view inherits task scope from the overview; **\***
returns to all checkpoints. No matching session leaves a policy field selected,
so another task's session is not accidentally offered as the selection to close.
The policy draft remains in the existing screen/panel objects throughout this
navigation. Overview refresh runs off the terminal event loop, keeps selection by
task ID, preserves scope/page and ignores responses from obsolete requests.
Refresh failures retain the last snapshot with a stale warning. Provider list
refreshes also keep selection by provider ID.

## Read model, boundaries and compatibility

The backend opens SQLite read-only/query-only and uses one short read transaction.
The default page has 25 tasks, with an invariant cap of 50; provider receipts are
bounded to 50. Filters/cursors accept 1–256-byte safe identifiers. These are query
and safety bounds, not new persisted configuration keys. All existing domain
settings remain editable through their shared catalogs in both clients.

The snapshot is **observed evidence**, never a task completion or health verdict:

- Memory totals reflect stored status. Actual retrieval revalidates scope,
  evidence freshness, conflicts and confidence.
- Lease records do not prove the process is alive. Existing fenced recovery owns
  that decision; the overview never reclaims a lease.
- Reconciliation counts consider only the newest checkpoint per role, avoiding
  alerts from superseded historical checkpoints.
- Provider receipts do not run detection, version probes, package installation,
  index builds or AI calls. Open Code Intelligence explicitly for live health.
- Latest context fields are estimates with their original measurement quality.
  No source text, whole PRD, transcript, capability token or private lease is
  included in the overview. Existing detail services retain their validation.

No SQLite migration, AI adapter configuration, new worker, durable policy field
or coordinator scheduling change is introduced. JSON/pre-O2 projects see a clear
unavailable state and links to existing migration/upgrade workflows; no migration
is performed implicitly. Existing advanced operator actions and domain pages
remain available. Native compaction capabilities and unsupported provider actions
are still reported by their original adapters.

This milestone consolidates the delivered workflows; it does not implement every
illustrative future route, provider feature, graph or benchmark in the design spec.
The overview uses snapshot polling with stale/retry feedback, rather than claiming
a new durable event-stream or a global task-delivery authority.

## Validation

Coverage includes Rust read-only snapshots, paging, exact filtering, joined
checkpoint/context references and suppression of old reconciliation alerts;
TUI task navigation, selection and draft preservation with terminal rendering;
Web task links, paging, stale/error states, obsolete-response isolation, domain
policy preservation and automated accessibility checks. Existing operator mutation
permission gates and agent allowlists remain unchanged.
