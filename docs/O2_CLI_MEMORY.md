# O2 settings and memory diagnostics from the CLI

These commands use the same field catalog, configuration transactions, memory
scopes and freshness checks as the other clients. They do not invoke AI tools.

## Edit one setting

```sh
macc o2 policy list
macc o2 policy get memory.enabled

# Preview only: no YAML write.
macc o2 policy set memory.enabled true

# In a terminal: show the change and ask for confirmation (default: cancel).
macc o2 policy set memory.enabled true --apply
```

Values follow the field's declared type: `true`/`false` for booleans, numbers
within the catalog range, or an unquoted enum/string argument. Quote executable
paths containing spaces using the shell's normal quoting rules. Unknown fields,
invalid types, ranges and dependent settings are rejected before writing.

For automation, preserve and review the preview, then confirm its revision:

```sh
macc o2 policy set memory.enabled true > preview.json
REVISION=$(jq -r '.revision' preview.json)
macc o2 policy set memory.enabled true \
  --apply --confirm --revision "$REVISION"
```

Non-interactive input never automatically grants confirmation. `--confirm`
requires both `--apply` and `--revision`. An intervening configuration change
rejects the edit, including a change while an interactive prompt is open. Only
the selected O2 group is written; other absent/inherited groups remain absent.

Memory proposals still require the reviewed SQLite activation described in
[O2 P1](O2_P1.md). Enabling `memory.enabled` does not migrate sessions or activate
proposed facts. Enable Memory before enabling Context Engine; provider deployment
and semantic-search adoption retain their separate gates.

## Explain missing search results

Default search remains the compact, token-budgeted response used by agents:

```sh
macc o2 memory-search "typecheck" --task TASK-1
```

Use an explicit operator explanation when a previously visible fact disappears:

```sh
macc o2 memory-search "typecheck" --task TASK-1 --explain
macc o2 memory-search "typecheck" --worktree /absolute/project/worktree --explain
```

The response includes:

- `records`: the same eligible, ranked results as a normal full search;
- `exclusions`: matching, in-scope facts withheld because of changed source
  evidence, inactive status, unresolved conflicts or insufficient confidence,
  plus disabled-memory/provider diagnostics where applicable;
- `candidate_limit`, `result_limit`, `exclusions_truncated` and a note describing
  the bounded scans.

An explanation does not promote facts, persist source staleness or reset memory.
It does not inspect or disclose other task/worktree scopes. Source restoration
can make a still-active fact eligible again without rewriting its review history.
Explicitly invalidated facts remain invalidated.

`--explain` is operator output outside the agent token budget and cannot be
combined with `--full` or `--cursor`. Existing `--all` remains an explicit request
to inspect inactive facts. Scope and query filters still apply. Diagnostics are
bounded, so an empty result is not proof that the whole database has no facts.
The typed `memory_explain`, `policy_field_get`, `policy_field_preview` and
`policy_field_apply` actions are also available through `macc o2 action` and the
existing O2 API; `policy_field_apply` requires `confirmed: true` and a revision.
