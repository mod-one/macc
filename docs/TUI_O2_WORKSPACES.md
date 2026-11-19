# Sessions and O2 in the terminal

## Open a workspace

The Home overview lists both destinations. Press **O** (uppercase) for **O2 Memory & Context** or **S** for **Sessions & Conversations**. Both also appear in the searchable command palette (**:**), and in contextual help (**?**). The older **Sessions → o** shortcut remains available.

Each workspace has its own title and breadcrumb. Opening O2 for the first time shows the task overview or explicit storage setup instructions. A missing PRD does not prevent inspecting these configuration screens.

## Find an operation

- **:** or **Ctrl+P** opens the shared searchable command palette for the active page or window. Type an action name, select with **Up/Down**, then **Enter**. The lower pane explains the selected action. Unavailable actions remain visible with an explanation; they do not execute.
- **?** opens contextual help. Scroll with **Up/Down** or **Page Up/Page Down**; **Esc** closes help.
- **Tab/Shift+Tab** changes section; number keys open sections directly.
- **Enter** edits the selected setting or inspects the selected record. The selected row stays visible while navigating.
- **Page Up/Page Down** scrolls the details pane. **Ctrl+R** switches between readable evidence and complete raw evidence.
- **Esc** cancels input or a pending reviewed operation before navigating back. Unsaved policy drafts require explicit save or discard.

The palette and keyboard invoke the same workspace actions and project ownership gate. A viewer cannot bypass the gate by choosing an operation with Enter. Core services still validate schema, scope, source hashes, revisions and confirmation checksums.

## Sessions

**Configure → Sessions** shows the session policy catalog in a table. The adjacent detail pane explains the selected field, its units, valid range and canonical key. **Enter** edits a value. **p** previews/validates the draft; **s** saves through the existing revision check. **x** discards a draft; defaults and inheritance are available through the command palette.

**Operate → Sessions** shows recorded task, role, tool, status and use count. An empty list explains when conversations are recorded. Select a row and press Enter for session/events evidence. Use `:` for usage, checkpoints, closure, lease recovery and backups. Ctrl+T opens settings and the reviewed storage migration. Closing a leased session remains forbidden.

Storage migration is always a reviewed action: prepare the plan, stop active coordinator/supervisor/runners, inspect backup/source evidence, then confirm with **a**. Esc cancels the plan without changing storage.

## O2

| Mode | Section | Purpose |
| --- | --- | --- |
| Operate | 1 Memory | Proposals, reviews, conflicts, import/export and retrieval evidence |
| Operate | 2 Context | Invocation context and output reduction |
| Operate | 3 Overview | Task-scoped sessions, checkpoints and proposals |
| Operate | 4 Trials | Paired policy trials and measured adoption |
| Configure | 1 Settings | Grouped O2 feature policies |
| Configure | 2 Compaction | Adapter probes, checkpoint and rotation operations |
| Configure | 3 Code intelligence | Code queries and reviewed provider operations |

**Tab**, **Shift+Tab** and the displayed numbers navigate within the mode. **Ctrl+T** switches modes. In **Configure → O2 → Settings**, use **[ / ]** to switch **Memory / Context / Compaction / Code intelligence**. Under Memory, **Left/Right** folds or expands Ledger, Semantic search, Ranking weights, MCP access and Limits. A search expands the matching group. Up/Down stays within that group; **/** finds a setting across the complete shared catalog. Enter toggles a boolean, cycles an enumerated choice or opens an exact-value form. **p** previews the current revision; **s** saves the previewed draft; **x** discards it.

Forms show one named step, its current value, Back/Next controls and explicit cancellation. A completed write form prepares a review rather than silently applying it. Inspect the operation, then **a** confirms; Esc discards it. Advanced JSON remains optional in the command palette.

JSON/older SQLite projects get a setup card instead of an unexplained empty panel. It links to Sessions for SQLite migration, O2 schema upgrade when needed, and Settings for feature activation. Neither opening the screen nor following setup navigation performs a migration, enables a feature, installs a provider or starts an AI call.

## Terminal sizes and persistence

The layouts keep list, details and action hints visible at **80×24** and expand at larger sizes. The action menu, forms and help use overlays; long evidence stays scrollable. Switching workspace/group retains drafts and read-only refresh does not silently rebase a stale settings revision.

No persistence schema, policy default, provider adapter or coordinator scheduling behavior changes in this UI update. Existing CLI/Web configuration and checkpoint evidence remain compatible.

See [TUI command conventions](TUI_COMMANDS.md) for the shared footer, modal behavior and shortcut scopes.
