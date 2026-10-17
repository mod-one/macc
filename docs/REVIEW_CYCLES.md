# Review and correction cycle budget

`automation.coordinator.max_review_cycles` counts allowed reviews together with
the correction requested by each review:

- `0`: skip new reviews. An existing rejected review is never merged by disabling review.
- `1`: one review and one correction if requested, with no second review.
- `N`: at most N reviews, each with its requested correction.
- unset: continue the review/correction loop without a fixed limit.

The existing `task_runtime.review_cycles` field continues to count completed
reviews. Reaching the configured limit stops scheduling another review; it does
not cancel the correction owed to the review just completed. This preserves
persisted counters, including interrupted tasks already marked changes_requested.

A successful correction re-enters validation, so enabled tests run on the changed
sources. After the last permitted correction succeeds and configured validation
passes, MACC queues a merge. A failed correction blocks instead. An approved
review makes a task ready for merge; only the actual merge marks it merged.

If a rejected task already has a counter above its current configured limit
(for example after lowering the setting), MACC blocks it and preserves its
worktree for recovery. Exhausting a budget never directly merges a rejection.
