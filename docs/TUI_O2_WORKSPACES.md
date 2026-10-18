# Sessions and O2 in the terminal

## Open a workspace

The Home overview lists both destinations. Press **O** (uppercase) for **O2 Memory & Context** or **J** for **Sessions & Conversations**. Both also appear in the searchable command palette (**F2**, with **:** outside text input), and in contextual help (**F1**). The older **Sessions → o** shortcut remains available.

Each workspace has its own title and breadcrumb. Opening O2 for the first time shows the task overview or explicit storage setup instructions. A missing PRD does not prevent inspecting these configuration screens.

## Find an operation

- **F2** opens the shared searchable command palette for the active page or window. Type an action name, select with **Up/Down**, then **Enter**. The lower pane explains the selected action. Unavailable actions remain visible with an explanation; they do not execute.
- **?** or **F1** opens contextual help. Scroll with **Up/Down** or **Page Up/Page Down**; **Esc** closes help.
- **Tab/Shift+Tab** changes section; number keys open sections directly.
- **Enter** edits the selected setting or inspects the selected record. The selected row stays visible while navigating.
- **Page Up/Page Down** scrolls the details pane. **F4** switches between readable evidence and complete raw evidence.
- **Esc** cancels input or a pending reviewed operation before navigating back. Unsaved policy drafts require explicit save or discard.

Palette actions resolve to the existing operation shortcuts **before** the project ownership gate runs. A viewer cannot bypass the gate by choosing an operation with Enter. Core services still validate schema, scope, source hashes, revisions and confirmation checksums.

## Sessions

**1 Settings** shows the session policy catalog in a table. The adjacent detail pane explains the selected field, its units, valid range and canonical key. **Enter** edits a value. **p** previews/validates the draft; **s** saves through the existing revision check. **x** discards a draft; defaults and inheritance are available through F2.

**2 Conversations** shows recorded task, role, tool, status and use count. An empty list explains when conversations are recorded. Select a row and press Enter for session/events evidence. Usage, checkpoints, closure preparation, lease recovery, consistent backup and reviewed storage migration are available through F2. Closing a leased session remains forbidden.

Storage migration is always a reviewed action: prepare the plan, stop active coordinator/supervisor/runners, inspect backup/source evidence, then confirm with **a**. Esc cancels the plan without changing storage.

## O2

| Section | Purpose |
| --- | --- |
| 1 Memory | Sourced proposals, reviews, conflicts, import/export and retrieval evidence |
| 2 Context | Recorded mandatory/optional invocation context and output reduction |
| 3 Compaction | Runtime decisions, adapter probes and reviewed checkpoint/rotation operations |
| 4 Code | Current-code queries and reviewed provider lifecycle operations |
| 5 Settings | Grouped configuration of optional O2 capabilities |
| 6 Overview | Task-scoped sessions, checkpoints, context and proposal counts |
| 7 Trials | Pre-registered paired policy trials and measured adoption gates |

In **5 Settings**, use **Left/Right** to switch **Memory / Context / Compaction / Code intelligence**. The group's main enable switch is shown first. Up/Down stays within that group; **/** finds a setting across the complete shared catalog. Enter toggles a boolean, cycles an enumerated choice or opens an exact-value form. **p** previews the current revision; **s** saves the previewed draft; **x** discards it.

Forms show one named step, its current value, Back/Next controls and explicit cancellation. A completed write form prepares a review rather than silently applying it. Inspect the operation, then **a** confirms; Esc discards it. Advanced JSON remains optional under F2.

JSON/older SQLite projects get a setup card instead of an unexplained empty panel. It links to Sessions for SQLite migration, O2 schema upgrade when needed, and Settings for feature activation. Neither opening the screen nor following setup navigation performs a migration, enables a feature, installs a provider or starts an AI call.

## Terminal sizes and persistence

The layouts keep list, details and action hints visible at **80×24** and expand at larger sizes. The action menu, forms and help use overlays; long evidence stays scrollable. Switching workspace/group retains drafts and read-only refresh does not silently rebase a stale settings revision.

No persistence schema, policy default, provider adapter or coordinator scheduling behavior changes in this UI update. Existing CLI/Web configuration and checkpoint evidence remain compatible.

See [TUI command conventions](TUI_COMMANDS.md) for the shared footer, modal behavior and shortcut scopes.
