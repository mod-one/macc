# TUI commands

Every page and window has a fixed **Commands** area at the bottom. Commands use **[key] Action**, for example **[u] Resume paused run**. The title preserves the current navigation path. Long action lists end with **[F2] All** instead of clipping a command halfway through its name.

| Key | Meaning |
| --- | --- |
| F1 | Help for the active page or window; scroll with arrows or Page Up/Down |
| F2 | Searchable command palette; press again or Esc to close |
| Esc | Cancel the active input/confirmation, close an overlay, or go back; quit from Home |
| Up/Down | Select an item or scroll the active list |
| Enter | Activate the selected item; confirmations retain their explicit submit step |
| Tab / Shift+Tab | Switch sections on configuration and workspace pages |
| : | Palette alias outside text fields |
| Ctrl-P | Palette alias, including while editing |
| ? | Help alias outside text fields where handled by the page |

## Find and execute an action

Press F2, type part of its name, use Up/Down, then Enter. The list scrolls to keep the selected command visible. Its explanation appears below the list. Unavailable actions stay visible with a label and cannot execute. Clearing the search shows the complete list again.

The palette includes the active page's commands and navigation destinations. Inside a modal or text editor it only exposes that context's commands, so navigation cannot bypass a confirmation or discard input. Closing the palette returns to the same field or window. Text characters, including `:` and `?`, stay in text fields; function keys do not alter the draft.

The footer, contextual help and palette use the same active command catalog. A selected action returns through the existing keyboard dispatcher, ownership checks and confirmation steps. Choosing a stop command still opens its confirmation.

## Shortcut scope

F1, F2 and Esc have consistent meanings. Letter shortcuts remain local to their page to preserve existing controls. For example, `u` resumes on Coordinator Live and stages inherited settings in Sessions. The current context and action name are always shown; use F2 when unsure.

Coordinator Live distinguishes **[R] Run coordinator** from **[r] Requeue selected task**. Lowercase `l` toggles the log pane. The palette labels these actions according to their actual handlers. Task actions are unavailable when no task is selected. Ownership requirements are still enforced when executing a coordinator action.

## Implementation

- `command_catalog.rs`: active context, availability and page bindings.
- `command_palette.rs`: search, scrolling and dispatch through existing handlers.
- `command_footer.rs`: persistent bottom area, rendered after modal overlays.
- Sessions and O2 keep domain action metadata in their own command catalogs; the shared surface consumes it.

No storage schema, tool adapter or coordinator execution policy changes are introduced by this command presentation layer.

## Apply and launch confirmations

Enter on Apply opens a separate confirmation. Use Left/Right or Tab to choose Cancel / Apply; user files also require `YES`. A changed plan needs another preview. Run opens an adaptive launch review with supervisor and reference-branch choices. See [client action reviews](CLIENT_ACTION_REVIEWS.md).
