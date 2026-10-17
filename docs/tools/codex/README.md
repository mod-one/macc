# Codex integration

MACC generates `.codex/config.toml` and `AGENTS.md`, and runs Codex through
`adapters/codex/codex.performer.sh`. The tool registry defines fresh calls as
`codex exec` and continuations as `codex exec resume <session_id>`.

## Configuration and permissions

- MACC consumes `context`, `rules_enabled`, selection lists, and `model_tiers`.
  These are not written as Codex settings. In particular, `context.protect`
  controls MACC context-file handling; it is not a Codex configuration key.
- Native `skills` and `agents` tables pass through. MACC selection lists and
  comma-separated selections with those names are removed.
- Legacy `profiles` are not emitted into project configuration. Configure
  user profiles with the format supported by the installed Codex version.
- The shipped performer commands use `--yolo`, which bypasses Codex's approval
  and sandbox settings for those invocations. Project settings require the
  project to be trusted by Codex.
- Model and reasoning effort are explicit CLI overrides. Tier routing changes
  these arguments for the current invocation without rewriting the persistent
  effort setting or any nested TOML table. The shared runner retains its
  config-file fallback for tools without an explicit effort override.

The current performer spec grants host access through `--yolo`. Docker socket
access still requires the operating-system permissions of the runner process;
the flag does not grant group membership.

After updating MACC, regenerate project configuration with `macc apply` and
refresh the worker configuration/runner before retrying. Already-generated
worker files and already-running processes do not change with this source patch.

## Resume and result handling

Codex resume failures return to the coordinator without an automatic second
invocation inside the adapter. A failed call can already have performed work;
silently rerunning it can repeat side effects and obscure the first error.
The coordinator retains responsibility for retry decisions.

The adapter preserves diagnostics and releases its session lease when it exits.
Task-result markers remain authoritative for task outcomes. A CLI exit status of
zero does not make `MACC_TASK_RESULT: error_without_changes` a successful task.
Quota diagnostics continue to produce `MACC_TOOL_LIMIT: quota_exhausted` even
when Codex returns zero.

`Custom tool call output is missing` is a Codex diagnostic about a missing tool
result. The supplied log does not establish why it went missing. These changes
do not repair Codex session history or claim to fix that internal diagnostic.
Keep the affected session ID and full runner log when investigating it.

## Verification

```bash
cargo test --manifest-path adapters/Cargo.toml -p macc-adapter-codex --locked
make test-tool-runner
make check
```

The shared runner target covers common performer behavior, Codex arguments and
failures, and Claude resume compatibility. The tests use fake executables and
shipped tool specifications. They exercise fresh runs, resumes, tier routing,
transport failures, quota errors, task results, and lease cleanup without an
API call. They require Bash, jq,
Python 3 with PyYAML, and `timeout`.
The Linux CI job and `make check` run these regressions.

Reviewed against Codex CLI 0.159.2 and official documentation on 2026-09-30:

- [Non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode)
- [Developer commands](https://learn.chatgpt.com/docs/developer-commands)
- [Configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)

`Codex_CLI_in_MACC_Plan.md` and `example.config.toml` are historical design
references, not the current CLI schema or an exhaustive list of supported keys.
