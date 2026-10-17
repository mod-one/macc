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
case "${SCENARIO:?}" in
  transport)
    echo 'ERROR: connection closed while resuming' >&2
    exit 23
    ;;
  quota)
    echo "ERROR: You've hit your usage limit." >&2
    exit 0
    ;;
  completed)
    echo 'MACC_TASK_RESULT: success_without_changes'
    exit 0
    ;;
  task_error)
    echo 'MACC_TASK_RESULT_EXP: Docker socket is unavailable.'
    echo 'MACC_TASK_RESULT: error_without_changes'
    exit 0
    ;;
esac
STUB
chmod +x "$WORK/bin/codex"
export PATH="$WORK/bin:$PATH"

SID=01a0c3f1-c956-7ab2-a9d9-78a7e85ccdc1
for attempt in 1 2; do
  for scenario in transport quota completed task_error; do
    log="$WORK/$attempt-$scenario.log"
    calls="$WORK/$attempt-$scenario.calls"
    rc=0
    (cd "$WORK/repo" && SCENARIO="$scenario" CALL_LOG="$calls" \
      timeout 30 bash "$REPO_ROOT/adapters/codex/codex.performer.sh" \
      --prompt-file "$WORK/prompt.txt" --tool-json "$WORK/repo/.macc/tool.json" \
      --repo "$WORK/repo" --worktree "$WORK/repo" --task-id T-1 \
      --attempt "$attempt" --max-attempts 3 --session-id "$SID") > "$log" 2>&1 || rc=$?
    expected=0
    case "$scenario" in transport) expected=23 ;; quota) expected=1 ;; esac
    if [[ "$rc" != "$expected" || "$(wc -l < "$calls")" != 1 ]]; then
      cat "$log"
      echo "FAIL: attempt=$attempt scenario=$scenario rc=$rc expected=$expected; expected one invocation" >&2
      exit 1
    fi
    case "$scenario" in
      transport) grep -q 'connection closed' "$log" ;;
      quota) grep -q 'MACC_TOOL_LIMIT: quota_exhausted' "$log" ;;
      completed) grep -q 'MACC_TASK_RESULT: success_without_changes' "$log" ;;
      task_error) grep -q 'MACC_TASK_RESULT: error_without_changes' "$log" ;;
    esac
    jq -e --arg sid "$SID" '.tools.codex.sessions[$sid].status == "available"' \
      "$WORK/repo/.macc/state/tool-sessions.json" >/dev/null
    echo "PASS: attempt=$attempt scenario=$scenario; one invocation, status and diagnostics preserved, lease released"
  done
done
