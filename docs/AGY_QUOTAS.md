# AGY quotas by model pool

MACC recognizes AGY's `Individual quota reached` response as `E602`
(`QuotaExhausted`), including `AGY_ERROR` output and runner limit markers.
A compact reset duration is parsed before diagnostic text truncation:
`Resets in 64h18m47s` means 231,527 seconds. Days and mixed-case units are also
accepted; overflow yields an unknown delay rather than a wrapped timestamp.
A generic transient 429 remains `E601`.

## Quota pools

The AGY adapter maps configured model IDs and display names to these pools:

| Model | Pool key |
| --- | --- |
| Gemini 4 Argon, all effort levels | `agy::gemini-4-argon` |
| Other Gemini models, including Gemini 3.8 Flash | `agy::gemini` |
| Claude and GPT models | `agy::claude-gpt` |

These mappings follow the AGY account groups observed in the supplied quota
screen. They are adapter policy, not provider names embedded in the coordinator.
An unknown model keeps conservative tool-wide throttling (`agy`). The error does
not expose remaining quota percentages or identify every overlapping account
limit. MACC does not claim that another pool has credit until its own invocation
is accepted. Effort variants within one pool do not get independent allowances.

## Dispatch and recovery

The coordinator resolves each task's model using its existing routing/tier and
tool configuration. It records that pool on the dispatched assignment, so an
error is attached to the model configuration used for the attempt. E602 stores
its deadline in the existing SQLite throttle registry under the pool key.

Dispatch checks both a tool-wide cooldown and the task's selected pool. A task
configured for Flash can remain eligible when Argon is exhausted. Changing only
Argon's effort level does not evade the cooldown. Generic rate limits remain
tool-wide. Tools without the pool capability retain their previous behavior,
including Codex and Claude.

MACC does not silently downgrade or replace the requested model. An exhausted
pool uses the existing authorized tool fallback, retaining the attached task
branch, pending edits and worktree. If no permitted fallback is available, the
coordinator waits for quota availability under its existing run timeout/stop
policy. Review/fix handoffs use the same pool constraints and preserve their work.

Pool keys and deadlines appear through the existing cooldown/status API and
CLI/TUI/Web cooldown views. Cooldowns survive restart and stop filtering tasks
when the provider's reset deadline has passed. A successful attempt in another
pool cannot clear the exhausted pool's entry. Existing tool-wide entries remain
conservative; no automatic migration guesses their original model scope.

## Scope and validation

No provider call, subscription change, account login or live project change is
needed to install this behavior. Regression tests cover AGY text/JSON and reset
durations, alias grouping, SQLite persistence, task selection across model pools,
worktree preservation and the existing Codex/Claude normalizers and runner flows.
