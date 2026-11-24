#!/usr/bin/env bash
# A Codex resume failure must reach the coordinator without a hidden second run.
set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/repo/.macc/state" "$WORK/bin"
python3 - "$REPO_ROOT/registry/tools.d/codex.tool.yaml" "$WORK/repo/.macc/tool.json" <<'PY'
import json, sys, yaml
spec = yaml.safe_load(open(sys.argv[1]))
def resolve(value):
    if isinstance(value, dict): return {k: resolve(v) for k, v in value.items()}
    if isinstance(value, list): return [resolve(v) for v in value]
    if isinstance(value, str): return value.replace("{model}", "test-model").replace("{effort}", "medium")
    return value
json.dump(resolve(spec), open(sys.argv[2], "w"))
PY
printf 'Complete the task\n' > "$WORK/prompt.txt"
cat > "$WORK/bin/codex" <<'STUB'
#!/usr/bin/env bash
printf 'invoked\n' >> "${CALL_LOG:?}"
while (($#)) && [[ "$1" != --output-last-message ]]; do shift; done
message="${2:?last-message path required}"
case "${SCENARIO:?}" in
  checkpoint_*)
    text="$ACK"
    kind=agent_message
    case "$SCENARIO" in
      checkpoint_tool) kind=command_execution ;;
      checkpoint_stale) text="$ACK-old" ;;
    esac
    event="$(jq -nc --arg t "$text" --arg kind "$kind" '{type:"item.completed",item:{type:$kind,text:($t+"\n\nStarting work.")}}')"
    if [[ "$SCENARIO" == checkpoint_stderr ]]; then echo "$event" >&2; else echo "$event"; fi
    if [[ "$SCENARIO" == checkpoint_failed ]]; then echo '{"type":"turn.failed","error":{"message":"Quota exceeded"}}'; fi
    echo 'MACC_TASK_RESULT: success_with_changes' > "$message"
    ;;
  transport)
    echo 'ERROR: connection closed while resuming' >&2
    exit 23
    ;;
  quota)
    echo "ERROR: You've hit your usage limit." >&2
    exit 0
    ;;
  quota_with_final|failed_turn)
    printf '%s\n' 'REVIEW_VERDICT: OK' 'MACC_TASK_RESULT: precondition_unmet' > "$message"
    if [[ "$SCENARIO" == failed_turn ]]; then
      echo '{"type":"turn.failed","error":{"message":"Quota exceeded"}}'
    else
      echo '{"type":"error","message":"Quota exceeded"}'
    fi
    ;;
  nested_error)
    echo '{"type":"item.completed","item":{"type":"command_execution","aggregated_output":"user\n{\"type\":\"error\",\"message\":\"Quota exceeded\"}"}}'
    echo 'REVIEW_VERDICT: OK' > "$message"
    ;;
  precondition)
    printf '%s\n' 'MACC_TASK_PRECONDITION: Attach device' 'MACC_TASK_RESULT: precondition_unmet' > "$message"
    exit 23
    ;;
  missing_final)
    echo 'assistant'
    echo 'MACC_DECISION_ACK: sha256:historical'
    echo 'REVIEW_VERDICT: OK'
    exit 0
    ;;
  completed)
    echo 'MACC_TASK_RESULT: success_without_changes' > "$message"
    exit 0
    ;;
  task_error)
    echo 'MACC_TASK_RESULT_EXP: Docker socket is unavailable.' > "$message"
    echo 'MACC_TASK_RESULT: error_without_changes' >> "$message"
    exit 0
    ;;
esac
STUB
chmod +x "$WORK/bin/codex"
export PATH="$WORK/bin:$PATH"
export MACC_PROTOCOL_CLI="${MACC_PROTOCOL_CLI:-$REPO_ROOT/target/debug/macc}"

source "$REPO_ROOT/automat/tests/session_fixture.sh"

SID=01a0c3f1-c956-7ab2-a9d9-78a7e85ccdc1
for attempt in 1 2; do
  for scenario in transport quota quota_with_final failed_turn nested_error completed task_error missing_final precondition; do
    seed_owned_session "$WORK/repo" "$WORK/repo/.macc/tool.json" codex T-1 "$SID"
    log="$WORK/$attempt-$scenario.log"
    calls="$WORK/$attempt-$scenario.calls"
    rc=0
    (cd "$WORK/repo" && SCENARIO="$scenario" CALL_LOG="$calls" \
      timeout 30 bash "$REPO_ROOT/adapters/codex/codex.performer.sh" \
      --prompt-file "$WORK/prompt.txt" --tool-json "$WORK/repo/.macc/tool.json" \
      --repo "$WORK/repo" --worktree "$WORK/repo" --task-id T-1 \
      --attempt "$attempt" --max-attempts 3 --session-id "$SID") > "$log" 2>&1 || rc=$?
    expected=0
    case "$scenario" in transport|precondition) expected=23 ;; quota|quota_with_final|failed_turn|missing_final) expected=1 ;; esac
    if [[ "$rc" != "$expected" || "$(wc -l < "$calls")" != 1 ]]; then
      cat "$log"
      echo "FAIL: attempt=$attempt scenario=$scenario rc=$rc expected=$expected; expected one invocation" >&2
      exit 1
    fi
    case "$scenario" in
      missing_final)
        grep -q 'Provider returned no final message' "$log"
        if grep -qE '^(MACC_DECISION_ACK|REVIEW_VERDICT):' "$log"; then exit 1; fi ;;
      quota_with_final|failed_turn)
        grep -q 'Quota exceeded' "$log"
        if grep -qE '^(MACC_TASK_RESULT|REVIEW_VERDICT):' "$log"; then exit 1; fi ;;
      nested_error) grep -qx 'REVIEW_VERDICT: OK' "$log" ;;
      transport) grep -q 'connection closed' "$log" ;;
      quota) grep -q 'hit your usage limit' "$log" ;;
      precondition) grep -q '^MACC_TASK_RESULT: precondition_unmet$' "$log" ;;
      completed) grep -q 'MACC_TASK_RESULT: success_without_changes' "$log" ;;
      task_error) grep -q 'MACC_TASK_RESULT: error_without_changes' "$log" ;;
    esac
    jq -e --arg sid "$SID" '.tools.codex.sessions[$sid].status == "available"' \
      "$WORK/repo/.macc/state/tool-sessions.json" >/dev/null
    echo "PASS: attempt=$attempt scenario=$scenario; one invocation, status and diagnostics preserved, lease released"
  done
done

# Drive fresh-session finalization through the same process and protocol readers.
export ACK='MACC_CHECKPOINT_ACK: checkpoint-1 task=T-1 phase=dev'
export FINISH="$WORK/finish.json"
export MACC_SESSION_SERVICE_BIN="$WORK/bin/test-session"
cat > "$MACC_SESSION_SERVICE_BIN" <<'SERVICE'
#!/usr/bin/env bash
set -eu
if [[ "$1" == runner-process ]]; then exec "$MACC_PROTOCOL_CLI" "$@"; fi
case "${4:?session operation}" in
  checkpoint-prepare) jq -nc --arg ack "$ACK" '{enabled:true,checkpoint_id:"checkpoint-1",acknowledgement:$ack,prompt:$ack}' ;;
  acquire) echo '{"session_id":"fixture","lease_token":"token","generation":1,"is_new":true,"heartbeat_seconds":60}' ;;
  checkpoint-finish) cat > "$FINISH"; echo '{}' ;;
  *) cat >/dev/null; echo '{}' ;;
esac
SERVICE
chmod +x "$MACC_SESSION_SERVICE_BIN"
jq -nc --arg root "$WORK/repo" '{session_store:{version:1,mode:"sqlite",project_root:$root}}' > "$WORK/repo/.macc/state/tool-sessions.json"
for scenario in checkpoint_valid checkpoint_tool checkpoint_stale checkpoint_stderr checkpoint_failed; do
  rc=0
  : > "$FINISH"
  SCENARIO="$scenario" CALL_LOG="$WORK/calls" bash "$REPO_ROOT/adapters/codex/codex.performer.sh" \
    --prompt-file "$WORK/prompt.txt" --tool-json "$WORK/repo/.macc/tool.json" \
    --repo "$WORK/repo" --worktree "$WORK/repo" --task-id T-1 > "$WORK/check.log" 2>&1 || rc=$?
  if [[ "$scenario" == checkpoint_valid ]]; then
    [[ "$rc" == 0 && -s "$FINISH" ]] && jq -e '.acknowledged and .result=="success"' "$FINISH" >/dev/null || { cat "$WORK/check.log"; exit 1; }
    grep -qx 'MACC_TASK_RESULT: success_with_changes' "$WORK/check.log"
  else
    [[ "$rc" != 0 && -s "$FINISH" ]] && jq -e '.acknowledged==false and .result!="success"' "$FINISH" >/dev/null || { cat "$WORK/check.log"; exit 1; }
  fi
  echo "PASS: $scenario"
done
