# Maximum reviews per task

`automation.coordinator.max_review_cycles` is a hard limit on reviewer attempts
for each task. Before dispatch, MACC reserves up to the configured number of
runner attempts from the remaining slots. When the phase returns, it records
each actual invocation and releases unused slots. A failed precheck that never
invoked the reviewer therefore uses zero slots. An attempt without a valid
verdict still counts. If the driver crashes before reporting usage, its
outstanding reservations remain charged across restarts to enforce the limit.

| Setting | Behavior |
| --- | --- |
| Unset | Unlimited reviewer attempts |
| 0 | No new reviews |
| 1 | One review and its correction if requested |
| 2 | Up to two reviews, each with its requested correction |

For example, if both reviews request changes, the second correction runs and the
configured tests run on the corrected source. The task detail shows
`2/2 reviews used · 2/2 corrections completed · no review verdict for the corrected revision`.
After the last slot, MACC skips any further review request, including a refresh
after a resumed correction, and queues the merge. A skipped review produces no
verdict. Pending decisions, failed checks, and Git publication errors can still
block delivery. Existing stops caused solely by the review limit in the review
phase are reconsidered on the next coordinator advance.

`task_runtime.review_cycles` counts completed verdicts. `task_runtime.review_usage`
records slots used, the limit at reservation, corrections completed, and the
review state. Older tasks retain their recorded review count. MACC omits the
correction total when its history is unknown. Automatic runner retries, explicit
retries and mandatory reviews all share the same slots.
