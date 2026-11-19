# MACC terminal client

Start with `macc tui`. **Operate** opens on Overview. Press **Ctrl+T** to switch to **Configure**. From Coordinator, Sessions, O2 Memory or Inbox, Ctrl+T opens the related settings. From Overview it opens Changes. At 105 columns and wider the rail lists destinations. On narrower terminals the same destinations are available by shortcut and by name in the command palette.

| Page key | Operate | Configure |
| --- | --- | --- |
| H | Overview | Changes |
| C | Coordinator Live | Coordinator settings |
| I | Inbox | |
| S | Sessions | Session settings |
| O | O2 Memory | O2 settings |
| L / D | Logs / Doctor | |
| G / T | | General / Tools |
| K / M / P | | Skills / MCP servers / Hooks |

**Uppercase letters navigate. Lowercase letters act on the current page.** Digits select visible sections. Arrows select rows, Enter opens or edits, and Esc goes back or cancels. `Ctrl+C` exits the TUI from any page, text field or dialog. Save configuration edits before quitting to keep them. `?` lists the active commands and their availability. `:` searches by name or description and executes the selected command. Actions without a physical shortcut can still be selected by name in the palette. In a text field, `?` and `:` type normally. Use `Ctrl+P` to open the palette while editing, including its Help command. In Configure, `u` undoes a configuration edit and `Ctrl+Z` reapplies the last undone edit. A modal exposes only its own operations. The bottom bar shows the most relevant commands in `[key] Action` format.

## Common workflows

- **Review readiness** on Overview. The first pending step is selected when readiness loads. Green ✓ marks a verified step, amber ▲ marks the next action, and · marks a later pending step. Select a step and press Enter to open its workspace. The Activity section shows project details and recent runs.
- **Start a run** with `r` on Overview or Coordinator Live. Review the launch options and confirm. On Live, `u` resumes, `t` requeues the selected task, `x` opens the task-stop confirmation, `l` toggles the log pane, and `i` opens the reported supervisor intervention when one exists. The Live table, selection details and last result or live logs are stacked vertically. Use ↑/↓ to select a worker or the pinned supervisor intervention. Enter inspects the selected row or opens its pending decision. Retry, stop and diff target worker tasks. The nine table fields remain available as labelled values on narrow terminals. HB shows the worker’s heartbeat age. On a supervisor row it shows time in the current intervention phase.
- **Respond to a request** in Operate → Inbox. Inspect pending decisions, approvals or history. Use Enter to open the existing response or approval form. The selected response still follows the normal evidence and approval workflow.
- **Diagnose a project** in Operate → Doctor. Select a finding to read its explanation and recommended action. Ctrl+R shows the underlying finding. Refresh with `r`. Checks run in the background. A recorded tool cooldown has its own row. Select it and press Enter to clear its local waiting period.
- **Configure the coordinator** with Configure → C. The sections are Run, Tool routing, Phases, Reliability, Supervisor, Decisions and Advanced (1–7). General settings have their own G destination.
- **Configure and deploy** in Configure → Changes. The draft count includes individual policy fields. The list shows the setting path and the old and new values for switches, numbers and recognized phase modes. Free-form values stay private. Enter opens the corresponding editor. Saved changes remain listed until a successful Apply in this session. Save the configuration, preview the managed file changes with `p`, and open Apply with `a`. O2 and session policies retain their own revision-checked save. A pending policy draft appears in Changes. Apply asks for confirmation and user-scope operations require the explicit consent phrase.
- **Inspect tools and hooks** in Configure. Tool settings and installation share the same inventory. Hooks separates MACC processors from native deployment. Installation, activation and observed execution have separate meanings. Operations that take effect immediately, such as installing tools, importing hooks, or confirming a storage migration, keep their own review.

## Command implementation

`command_catalog` builds the active bindings from typed terminal keys, actions, labels and availability. Navigation, palette selection, help and the footer consume these descriptors. The dispatcher invokes semantic page actions directly. Focused editors accept text and widget events in their own context. A specialist operation whose key is reserved for navigation remains available by name.

Tests in `tui/src/command_tests.rs` check binding uniqueness, keyboard/palette agreement, modal isolation, mode switching and disabled actions. The workspace and decision tests exercise actual Ratatui buffers on compact and wide terminals. Run `cargo test --locked -p macc-tui`.
