#!/usr/bin/env bash
# Real Git fixtures; no live model calls.
set -euo pipefail
AUTOMAT="$(cd "$(dirname "$0")/.." && pwd)"
source "$AUTOMAT/completion_evidence.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"
git init -q -b main
git config user.name 'MACC Test'
git config user.email 'macc@example.invalid'
printf '.macc/\nworktree.prd.json\nperformer.sh\n' > .gitignore
git add .
git commit -qm init
base_ref=main
tool=fake
set_last_error() { LAST_ERROR_CODE="$1"; LAST_ERROR_MESSAGE="$3"; }
soft_emit_performer_event() { :; }
spec='{"id":"T-1","acceptance_criteria":["existing implementation tested"]}'
# Missing evidence cannot become a durable success.
if record_completion_evidence T-1 title already_satisfied '' "$spec"; then exit 1; fi
[[ "$LAST_ERROR_CODE" == E904 ]]
git checkout -qb ai/fake/task
before="$(git rev-parse HEAD)"
record_completion_evidence T-1 title already_satisfied 'Acceptance criterion checked; targeted test passed.' "$spec"
[[ "$COMPLETION_EVIDENCE_CREATED" == true ]]
sha="$COMPLETION_COMMIT_SHA"
[[ "$sha" != "$before" ]]
[[ -z "$(git diff "$before" "$sha" --name-only)" ]]
git log -1 --format=%B | grep -Fq '[macc:result already_satisfied]'
git log -1 --format=%B | grep -Fq 'targeted test passed'
# A terminal-event retry must not generate duplicate proof commits.
record_completion_evidence T-1 title already_satisfied 'Acceptance criterion checked again.' "$spec"
[[ "$COMPLETION_COMMIT_SHA" == "$sha" ]]
# Publication makes the existing reference commit reusable.
git checkout -q main
git merge -q --ff-only ai/fake/task
git checkout -qb ai/fake/next
record_completion_evidence T-1 title already_satisfied 'Acceptance criterion still checked.' "$spec"
[[ "$COMPLETION_EVIDENCE_CREATED" == false ]]
[[ "$COMPLETION_COMMIT_SHA" == "$sha" ]]
[[ "$(git rev-parse HEAD)" == "$sha" ]]
# Contradictory source edits must not be hidden by an empty validation commit.
printf 'unexpected\n' > unrelated.txt
if record_completion_evidence T-2 title success_without_changes 'Validated.' '{"id":"T-2"}'; then exit 1; fi
[[ "$LAST_ERROR_CODE" == E904 ]]
rm unrelated.txt
# Commit-hook rejection is an error, never a successful terminal result.
printf '#!/bin/sh\nexit 1\n' > .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
if record_completion_evidence T-2 title already_satisfied 'Validated.' '{"id":"T-2"}'; then exit 1; fi
[[ "$LAST_ERROR_CODE" == E201 ]]
[[ "$(git rev-parse HEAD)" == "$sha" ]]
# A passed worktree PRD without a Git delivery is not sufficient evidence.
if record_completion_evidence T-3 title already_satisfied 'Previously marked passed' '{"id":"T-3"}' true; then exit 1; fi
[[ "$LAST_ERROR_CODE" == E904 ]]

rm .git/hooks/pre-commit
# An unpublished branch tip must not disappear behind an older task delivery.
git commit --allow-empty -qm 'Unpublished revision'
record_completion_evidence T-1 title already_satisfied 'Acceptance checked on unpublished revision.' "$spec"
[[ "$COMPLETION_EVIDENCE_CREATED" == true ]]
[[ "$(git rev-parse HEAD)" != "$sha" ]]
# Gate decisions are durable and retries preserve the latest decision.
gate_spec='{"id":"GATE","gate":{"required_verdict":"accepted"}}'
if record_completion_evidence GATE title already_satisfied 'Gate checked.' "$gate_spec"; then exit 1; fi
[[ "$LAST_ERROR_CODE" == E904 ]]
record_completion_evidence GATE title already_satisfied 'Gate check rejected.' "$gate_spec" false rejected
git log -1 --format=%B | grep -Fq '[macc:gate_verdict rejected]'
rejected_sha="$COMPLETION_COMMIT_SHA"
record_completion_evidence GATE title already_satisfied 'Gate check accepted.' "$gate_spec" false accepted
[[ "$COMPLETION_COMMIT_SHA" != "$rejected_sha" ]]
git log -1 --format=%B | grep -Fq '[macc:gate_verdict accepted]'
# Exercise the real runner success transaction with a deterministic tool stub.
# Terminal success must follow commit creation and select the merge lane.
eval "$(sed -n '/^run_tool()/,/^}/p' "$AUTOMAT/performer.sh")"
for fn in extract_task_result_marker extract_task_result_exp extract_task_gate_verdict resolve_task_result_exp validate_terminal_result_contract detect_success_result_kind; do
  eval "$(sed -n "/^${fn}()/,/^}/p" "$AUTOMAT/performer.sh")"
done
mkdir -p .macc
cat > .macc/fake-tool <<'RUNNER'
#!/usr/bin/env bash
printf 'MACC_TASK_RESULT_EXP: Existing acceptance criterion checked; test passed.\nMACC_TASK_RESULT: already_satisfied\n'
RUNNER
chmod +x .macc/fake-tool
tool_runner_path() { printf '%s' "$PWD/.macc/fake-tool"; }
log_debug_line() { :; }
log_task_line() { :; }
emit_performer_event() { :; }
spinner_start() { :; }
spinner_stop() { :; }
heartbeat_start() { :; }
heartbeat_stop() { :; }
must_emit_performer_event() {
  [[ "$1" == phase_result && "$3" == done ]]
  local proof
  proof="$(jq -r .completion_commit_sha <<< "$4")"
  git cat-file -e "${proof}^{commit}"
  printf '%s' "$4" > .macc/terminal-payload
}
performer_session_id=""
performer_session_enabled=false
CURRENT_PHASE=dev
LAST_ERROR_CODE=""
tool_json=.macc/tool.json
repo="$PWD"
worktree="$PWD"
task_log_file=.macc/task.log
task_id=TRANSACTION
next_title='Verified existing task'
next_task_json='{"id":"TRANSACTION"}'
if ! run_tool .macc/prompt 1 1; then exit 1; fi
jq -e '.changed == true and .result_kind == "success_with_changes" and .reported_result_kind == "already_satisfied"' .macc/terminal-payload >/dev/null
transaction_sha="$(jq -r .completion_commit_sha .macc/terminal-payload)"
git checkout -q main
git merge -q --ff-only ai/fake/next
if ! run_tool .macc/prompt 1 1; then exit 1; fi
jq -e '.changed == false and .result_kind == "already_satisfied"' .macc/terminal-payload >/dev/null
[[ "$(jq -r .completion_commit_sha .macc/terminal-payload)" == "$transaction_sha" ]]
# A missing explanation must never publish a terminal success.
cat > .macc/fake-tool <<'RUNNER'
#!/usr/bin/env bash
printf 'MACC_TASK_RESULT: already_satisfied\n'
RUNNER
rm .macc/terminal-payload
task_id=MISSING
next_task_json='{"id":"MISSING"}'
if run_tool .macc/prompt 1 1; then exit 1; fi
[[ "$LAST_ERROR_CODE" == E904 && ! -e .macc/terminal-payload ]]
printf 'Completion evidence tests passed.\n'  
