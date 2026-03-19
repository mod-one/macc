#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  performer.sh --repo <path> --worktree <path> --task-id <id> --tool <tool> --registry <path> --prd <path>

Env vars:
  PERFORMER_MAX_ITERATIONS  Max tasks to run before stopping (default: 50)
  PERFORMER_TOOL_MAX_ATTEMPTS Max attempts per task (default: 2)
  PERFORMER_SLEEP_SECONDS   Pause between tasks (default: 2)
EOF
}

repo=""
worktree=""
task_id=""
tool=""
registry=""
prd=""
performer_log_dir=""
task_log_file=""
EVENT_FILE="${COORD_EVENTS_FILE:-}"
EVENT_IPC_ADDR="${MACC_COORDINATOR_IPC_ADDR:-}"
EVENT_SOURCE="${MACC_EVENT_SOURCE:-}"
EVENT_TASK_ID="${MACC_EVENT_TASK_ID:-}"
EVENT_RUN_ID="${COORDINATOR_RUN_ID:-$(date +%s%N)-$$}"
EVENT_SEQ=0
EVENT_SEQ_FILE=""
LAST_IPC_ERROR=""
HEARTBEAT_PID=""
CURRENT_PHASE="dev"
LAST_ERROR_CODE=""
LAST_ERROR_ORIGIN=""
LAST_ERROR_MESSAGE=""
TERMINAL_EVENT_EMITTED="false"

PERFORMER_MAX_ITERATIONS="${PERFORMER_MAX_ITERATIONS:-50}"
PERFORMER_TOOL_MAX_ATTEMPTS="${PERFORMER_TOOL_MAX_ATTEMPTS:-2}"
PERFORMER_SLEEP_SECONDS="${PERFORMER_SLEEP_SECONDS:-2}"
PERFORMER_SPINNER="${PERFORMER_SPINNER:-true}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) repo="$2"; shift 2 ;;
    --worktree) worktree="$2"; shift 2 ;;
    --task-id) task_id="$2"; shift 2 ;;
    --tool) tool="$2"; shift 2 ;;
    --registry) registry="$2"; shift 2 ;;
    --prd) prd="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 1 ;;
  esac
done

if [[ -n "$EVENT_TASK_ID" && -n "$task_id" && "$EVENT_TASK_ID" != "$task_id" ]]; then
  LAST_ERROR_CODE="E901"
  LAST_ERROR_ORIGIN="performer"
  LAST_ERROR_MESSAGE="event task id mismatch"
  echo "Error: MACC_EVENT_TASK_ID mismatch. env=$EVENT_TASK_ID arg=$task_id" >&2
  exit 1
fi

if [[ -z "$EVENT_SOURCE" ]]; then
  EVENT_SOURCE="performer:${tool:-unknown}:${EVENT_RUN_ID}"
fi
if [[ -z "$EVENT_TASK_ID" ]]; then
  EVENT_TASK_ID="${task_id:-unknown}"
fi

if [[ -z "$repo" || -z "$worktree" || -z "$task_id" || -z "$tool" || -z "$registry" || -z "$prd" ]]; then
  LAST_ERROR_CODE="E901"
  LAST_ERROR_ORIGIN="performer"
  LAST_ERROR_MESSAGE="missing required args"
  echo "Error: missing required args" >&2
  usage
  exit 1
fi

if [[ ! -d "$worktree" ]]; then
  LAST_ERROR_CODE="E301"
  LAST_ERROR_ORIGIN="performer"
  LAST_ERROR_MESSAGE="worktree path does not exist"
  echo "Error: worktree path does not exist: $worktree" >&2
  exit 1
fi

if [[ ! -f "$prd" ]]; then
  LAST_ERROR_CODE="E302"
  LAST_ERROR_ORIGIN="performer"
  LAST_ERROR_MESSAGE="PRD file not found"
  echo "Error: PRD file not found: $prd" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  LAST_ERROR_CODE="E901"
  LAST_ERROR_ORIGIN="performer"
  LAST_ERROR_MESSAGE="jq is required"
  echo "Error: jq is required" >&2
  exit 1
fi

cd "$worktree"

tool_json="${worktree}/.macc/tool.json"
worktree_meta="${worktree}/.macc/worktree.json"

if [[ ! -f "$tool_json" ]]; then
  LAST_ERROR_CODE="E303"
  LAST_ERROR_ORIGIN="performer"
  LAST_ERROR_MESSAGE="tool.json not found"
  echo "Error: tool.json not found in worktree: $tool_json" >&2
  exit 1
fi

if [[ ! -f "$worktree_meta" ]]; then
  LAST_ERROR_CODE="E304"
  LAST_ERROR_ORIGIN="performer"
  LAST_ERROR_MESSAGE="worktree metadata not found"
  echo "Error: worktree metadata not found in worktree: $worktree_meta" >&2
  exit 1
fi

expected_branch="$(jq -r '.branch // ""' "$worktree_meta")"
current_branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
if [[ -z "$expected_branch" ]]; then
  LAST_ERROR_CODE="E305"
  LAST_ERROR_ORIGIN="performer"
  LAST_ERROR_MESSAGE="expected branch missing from worktree metadata"
  echo "Error: expected branch missing from worktree metadata: $worktree_meta" >&2
  exit 1
fi
if [[ -z "$current_branch" || "$current_branch" != "$expected_branch" ]]; then
  LAST_ERROR_CODE="E306"
  LAST_ERROR_ORIGIN="git"
  LAST_ERROR_MESSAGE="worktree branch mismatch expected=${expected_branch} actual=${current_branch:-unknown}"
  echo "Error: worktree branch mismatch. expected=$expected_branch actual=${current_branch:-unknown}" >&2
  exit 1
fi

if [[ -n "$EVENT_FILE" ]]; then
  EVENT_SEQ_FILE="${worktree}/.macc/tmp/event-seq-${EVENT_RUN_ID}.txt"
  mkdir -p "$(dirname "$EVENT_SEQ_FILE")"
  printf '0\n' >"$EVENT_SEQ_FILE"
fi

on_exit() {
  local rc=$?
  heartbeat_stop
  if [[ "$rc" -ne 0 && "$TERMINAL_EVENT_EMITTED" != "true" ]]; then
    emit_performer_event "failed" "$CURRENT_PHASE" "failed" "$(build_error_payload "$rc")"
    TERMINAL_EVENT_EMITTED="true"
  fi
  if [[ -n "${EVENT_SEQ_FILE:-}" ]]; then
    rm -f "$EVENT_SEQ_FILE" "${EVENT_SEQ_FILE}.lock" >/dev/null 2>&1 || true
  fi
}
trap on_exit EXIT

performer_log_dir="${worktree}/.macc/log/performer"
mkdir -p "$performer_log_dir"

task_log_path() {
  local id="$1"
  local safe
  safe="$(echo "$id" | tr '[:space:]' '-' | tr -cd '[:alnum:]_.-')"
  if [[ -z "$safe" ]]; then
    safe="task"
  fi
  echo "${performer_log_dir}/${safe}.md"
}

log_task_header_if_needed() {
  local path="$1"
  local id="$2"
  local title="$3"
  if [[ ! -f "$path" ]]; then
    cat >"$path" <<EOF
# Performer log for task ${id}

- Tool: ${tool}
- Worktree: ${worktree}
- PRD: ${prd}

EOF
  fi
}

log_task_line() {
  local msg="$1"
  if [[ -n "$task_log_file" ]]; then
    printf '%s\n' "$msg" >>"$task_log_file"
  fi
}

next_event_seq() {
  if [[ -z "${EVENT_SEQ_FILE:-}" ]]; then
    EVENT_SEQ=$((EVENT_SEQ + 1))
    echo "$EVENT_SEQ"
    return 0
  fi

  local lock_file="${EVENT_SEQ_FILE}.lock"
  local current=0
  while ! mkdir "$lock_file" 2>/dev/null; do
    sleep 0.01
  done
  if [[ -f "$EVENT_SEQ_FILE" ]]; then
    current="$(cat "$EVENT_SEQ_FILE" 2>/dev/null || echo 0)"
  fi
  [[ "$current" =~ ^[0-9]+$ ]] || current=0
  current=$((current + 1))
  printf '%s\n' "$current" >"$EVENT_SEQ_FILE"
  rmdir "$lock_file" >/dev/null 2>&1 || true
  echo "$current"
}

ipc_addr_display() {
  local addr="${EVENT_IPC_ADDR:-}"
  if [[ -n "$addr" ]]; then
    printf '%s' "$addr"
  else
    printf '%s' "<unset>"
  fi
}

ipc_event_preview() {
  local event_line="$1"
  local preview=""
  preview="$(
    jq -r '
      "type=\(.type // "<missing>") event_id=\(.event_id // "<missing>") status=\(.status // "<missing>") phase=\(.phase // "<missing>") has_result_kind=\((((.payload.result_kind? // "") | tostring | length) > 0))"
    ' <<<"$event_line" 2>/dev/null
  )"
  if [[ -n "$preview" ]]; then
    printf '%s' "$preview"
  else
    printf '%s' 'type=<parse_failed> event_id=<unknown> status=<unknown> phase=<unknown> has_result_kind=<unknown>'
  fi
}

emit_performer_event() {
  local event_type="$1"
  local phase="${2:-}"
  local status="${3:-}"
  local payload_json="${4:-}"
  [[ -n "$EVENT_FILE" || -n "$EVENT_IPC_ADDR" ]] || return 0
  [[ -n "$EVENT_SOURCE" ]] || EVENT_SOURCE="performer:${tool}:${EVENT_RUN_ID}"
  [[ -n "$EVENT_TASK_ID" ]] || EVENT_TASK_ID="$task_id"
  local seq
  seq="$(next_event_seq)"
  if [[ -z "$payload_json" ]]; then payload_json="{}"; elif ! jq -e 'type == "object"' <<<"$payload_json" >/dev/null 2>&1; then
    payload_json="$(jq -nc --arg value "$payload_json" '{value:$value}')"
  fi
  local event_line=""
  local jq_err_file=""
  local jq_err=""
  jq_err_file="$(mktemp)"
  if ! event_line="$(jq -nc \
    --arg schema_version "1" \
    --arg event_id "${EVENT_TASK_ID}-${seq}-$(date +%s%N)" \
    --arg run_id "$EVENT_RUN_ID" \
    --argjson seq "$seq" \
    --arg ts "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
    --arg source "$EVENT_SOURCE" \
    --arg task_id "$EVENT_TASK_ID" \
    --arg type "$event_type" \
    --arg phase "$phase" \
    --arg status "$status" \
    --argjson payload "$payload_json" \
    '{
      schema_version:$schema_version,
      event_id:$event_id,
      run_id:$run_id,
      seq:$seq,
      ts:$ts,
      source:$source,
      task_id:$task_id,
      type:$type,
      phase:($phase|select(length>0)),
      status:$status,
      payload:$payload
    }' 2>"$jq_err_file")"; then
    jq_err="$(tr '\n' ' ' <"$jq_err_file" | sed 's/[[:space:]]\\+/ /g')"
    rm -f "$jq_err_file"
    LAST_IPC_ERROR="event json build failed: addr=$(ipc_addr_display) type=${event_type} status=${status} phase=${phase:-<empty>} jq_stderr=${jq_err:-<empty>}"
    return 1
  fi
  rm -f "$jq_err_file"
  if [[ -z "$event_line" ]]; then
    LAST_IPC_ERROR="empty event json: addr=$(ipc_addr_display) type=${event_type} status=${status} phase=${phase:-<empty>}"
    return 1
  fi
  if [[ -n "$EVENT_IPC_ADDR" ]] && send_event_via_ipc "$event_line"; then
    return 0
  fi
  if [[ -n "$EVENT_FILE" ]]; then
    printf '%s\n' "$event_line" >>"$EVENT_FILE" 2>/dev/null || true
    return 0
  fi
  return 1
}

send_event_via_ipc() {
  local event_line="$1"
  local addr_display=""
  local preview=""
  local host="${EVENT_IPC_ADDR%:*}"
  local port="${EVENT_IPC_ADDR##*:}"
  local event_id=""
  local ack_line=""
  local ack_ok=""
  local ack_event_id=""
  LAST_IPC_ERROR=""
  addr_display="$(ipc_addr_display)"
  preview="$(ipc_event_preview "$event_line")"
  if [[ -z "$host" || -z "$port" || "$host" == "$port" ]]; then
    LAST_IPC_ERROR="invalid ipc addr: addr=${addr_display} event_id_extracted=false preview=\"${preview}\""
    return 1
  fi
  event_id="$(jq -r '.event_id // empty' <<<"$event_line" 2>/dev/null)"
  if [[ -z "$event_id" ]]; then
    LAST_IPC_ERROR="missing event_id: addr=${addr_display} event_id_extracted=false preview=\"${preview}\""
    return 1
  fi
  (
    exec 9<>"/dev/tcp/${host}/${port}" || exit 1
    printf '%s\n' "$event_line" >&9 || exit 1
    IFS= read -r -t 2 ack_line <&9 || exit 1
    ack_ok="$(jq -r '.ok // false' <<<"$ack_line" 2>/dev/null)" || exit 1
    ack_event_id="$(jq -r '.event_id // empty' <<<"$ack_line" 2>/dev/null)" || exit 1
    [[ "$ack_ok" == "true" && "$ack_event_id" == "$event_id" ]] || exit 1
    exec 9>&- 9<&-
  ) >/dev/null 2>&1
  local rc=$?
  if [[ $rc -eq 0 ]]; then
    return 0
  fi
  if command -v python3 >/dev/null 2>&1; then
    local py_err=""
    py_err="$(python3 - "$host" "$port" "$event_line" 2>&1 <<'PY'
import json, socket, sys
host, port, payload = sys.argv[1], int(sys.argv[2]), sys.argv[3]
event_id = json.loads(payload).get("event_id", "")
if not event_id:
    print("missing event_id")
    raise SystemExit(1)
with socket.create_connection((host, port), timeout=2) as sock:
    sock.sendall(payload.encode("utf-8") + b"\n")
    sock.settimeout(2)
    ack = b""
    while not ack.endswith(b"\n"):
        chunk = sock.recv(4096)
        if not chunk:
            print("no ack received")
            raise SystemExit(1)
        ack += chunk
try:
    ack_payload = json.loads(ack.decode("utf-8").strip())
except Exception as exc:
    print(f"ack parse error: {exc}")
    raise SystemExit(1)
if not ack_payload.get("ok"):
    print(f"ack negative: {ack_payload}")
    raise SystemExit(1)
if ack_payload.get("event_id") != event_id:
    print(f"ack event_id mismatch: {ack_payload.get('event_id')} != {event_id}")
    raise SystemExit(1)
PY
)"
    local py_rc=$?
    if [[ $py_rc -ne 0 ]]; then
      LAST_IPC_ERROR="python ipc failed: addr=${addr_display} event_id_extracted=true preview=\"${preview}\" detail=${py_err//$'\n'/ }"
    fi
    return $py_rc
  fi
  LAST_IPC_ERROR="tcp ipc failed: addr=${addr_display} event_id_extracted=true preview=\"${preview}\""
  return $rc
}

must_emit_performer_event() {
  local event_type="$1"
  local phase="${2:-}"
  local status="${3:-}"
  local payload_json="${4:-}"
  if emit_performer_event "$event_type" "$phase" "$status" "$payload_json"; then
    return 0
  fi
  if [[ -n "$EVENT_IPC_ADDR" ]]; then
    local source="${EVENT_SOURCE:-performer:${tool:-unknown}:${EVENT_RUN_ID}}"
    local detail="Error: failed to persist performer event via coordinator IPC: type=${event_type} task=${EVENT_TASK_ID:-$task_id} source=${source}"
    if [[ -n "$LAST_IPC_ERROR" ]]; then
      detail="${detail} error=${LAST_IPC_ERROR}"
    fi
    echo "$detail" >&2
    log_task_line "- ${detail}"
    return 1
  fi
  return 0
}

soft_emit_performer_event() {
  local event_type="$1"
  local phase="${2:-}"
  local status="${3:-}"
  local payload_json="${4:-}"
  if emit_performer_event "$event_type" "$phase" "$status" "$payload_json"; then
    return 0
  fi
  local source="${EVENT_SOURCE:-performer:${tool:-unknown}:${EVENT_RUN_ID}}"
  local detail="failed to persist non-terminal performer event: type=${event_type} task=${EVENT_TASK_ID:-$task_id} source=${source}"
  if [[ -n "$LAST_IPC_ERROR" ]]; then
    detail="${detail} error=${LAST_IPC_ERROR}"
  fi
  echo "Warning: ${detail}" >&2
  log_task_line "- Warning: ${detail}"
  return 0
}

set_last_error() {
  local code="$1"
  local origin="$2"
  local message="$3"
  LAST_ERROR_CODE="$code"
  LAST_ERROR_ORIGIN="$origin"
  LAST_ERROR_MESSAGE="$message"
}

build_error_payload() {
  local exit_code="$1"
  if ! command -v jq >/dev/null 2>&1; then
    printf '{"exit_code":%s}' "${exit_code:-0}"
    return 0
  fi
  jq -nc \
    --arg code "$LAST_ERROR_CODE" \
    --arg origin "$LAST_ERROR_ORIGIN" \
    --arg msg "$LAST_ERROR_MESSAGE" \
    --arg exit "$exit_code" \
    '{
      exit_code:($exit|tonumber?),
      error_code:($code|select(length>0)),
      origin:($origin|select(length>0)),
      message:($msg|select(length>0))
    }'
}

heartbeat_start() {
  [[ -n "$EVENT_FILE" || -n "$EVENT_IPC_ADDR" ]] || return 0
  heartbeat_stop
  (
    while true; do
      soft_emit_performer_event "heartbeat" "$CURRENT_PHASE" "running" '{}'
      sleep 2
    done
  ) &
  HEARTBEAT_PID=$!
}

heartbeat_stop() {
  if [[ -n "${HEARTBEAT_PID:-}" ]]; then
    kill "$HEARTBEAT_PID" >/dev/null 2>&1 || true
    wait "$HEARTBEAT_PID" >/dev/null 2>&1 || true
    HEARTBEAT_PID=""
  fi
}

spinner_enabled() {
  if [[ -n "${CI:-}" || -n "${MACC_NO_SPINNER:-}" ]]; then
    return 1
  fi
  if [[ "${PERFORMER_SPINNER}" != "true" ]]; then
    return 1
  fi
  [[ -t 2 ]]
}

spinner_start() {
  local msg="$1"
  if ! spinner_enabled; then
    return 0
  fi
  SPINNER_MSG="$msg"
  (
    local frames='|/-\'
    local i=0
    while true; do
      local ch="${frames:i%4:1}"
      printf '\r[%s] %s' "$ch" "$SPINNER_MSG" >&2
      i=$((i + 1))
      sleep 0.1
    done
  ) &
  SPINNER_PID=$!
}

spinner_stop() {
  local msg="$1"
  if [[ -n "${SPINNER_PID:-}" ]]; then
    kill "$SPINNER_PID" >/dev/null 2>&1 || true
    wait "$SPINNER_PID" >/dev/null 2>&1 || true
    SPINNER_PID=""
    if spinner_enabled; then
      printf '\r[done] %s\n' "$msg" >&2
    fi
  fi
}

tool_runner_path() {
  local runner
  runner="$(jq -r '.performer.runner // ""' "$tool_json")"
  if [[ -z "$runner" || "$runner" == "null" ]]; then
    echo ""
    return
  fi
  if [[ "$runner" = /* ]]; then
    echo "$runner"
  else
    echo "${repo}/${runner}"
  fi
}

JQ_ITEMS='
def task_items:
  if type == "array" then .
  elif type == "object" then (.tasks // .userStories // [])
  else []
  end;
task_items
'

get_next_task_json() {
  jq -c "${JQ_ITEMS} | map(select(.passes != true)) | .[0] // empty" "$prd"
}

get_next_task_id() {
  jq -r "${JQ_ITEMS} | map(select(.passes != true)) | .[0].id // \"\"" "$prd"
}

get_next_task_title() {
  jq -r "${JQ_ITEMS} | map(select(.passes != true)) | .[0].title // \"\"" "$prd"
}

mark_task_passed() {
  local id="$1"
  local tmp
  tmp="$(mktemp)"
  jq --arg id "$id" '
    def match_id($t):
      (($t.id|tostring) == $id);
    if type == "array" then
      map(if match_id(.) then .passes = true else . end)
    elif type == "object" then
      (if ((.tasks | type) == "array") then
         .tasks |= map(if match_id(.) then .passes = true else . end)
       else
         .
       end)
      | (if ((.userStories | type) == "array") then
           .userStories |= map(if match_id(.) then .passes = true else . end)
         else
           .
         end)
    else
      .
    end
  ' "$prd" >"$tmp"
  mv "$tmp" "$prd"
}

pending_task_count() {
  jq -r "${JQ_ITEMS} | map(select(.passes != true)) | length" "$prd"
}

build_prompt() {
  local task_json="$1"
  local task_id="$2"
  local task_title="$3"
  cat <<PROMPT
You are an autonomous coding agent working inside a MACC worktree.

Context:
- Worktree: ${worktree}
- Task file: ${prd}
- Task ID: ${task_id}
- Task Title: ${task_title}

Task (JSON):
${task_json}

Instructions:
1) Implement ONLY the task above.
2) Do NOT edit ${prd}; the runner will update it.
3) Do NOT commit; the runner will commit if all tasks are done.
4) Keep output concise; avoid dumping large files.
5) If the task acceptance criteria are already satisfied before any code change, this is a valid success. Verify it explicitly and do not make unnecessary edits.
6) At the end, print exactly one terminal result marker on its own line:
   - MACC_TASK_RESULT: success_with_changes
   - MACC_TASK_RESULT: success_without_changes
   - MACC_TASK_RESULT: already_satisfied
7) Use already_satisfied only when you verified the task is already done and can cite the evidence briefly.
8) If you finish successfully but forget the marker, the runner will infer the result from repository state; still print the marker explicitly.

Now implement the task.
PROMPT
}

extract_task_result_marker() {
  local output_file="$1"
  local raw=""
  raw="$(grep -E 'MACC_TASK_RESULT:' "$output_file" | tail -n 1 | sed -E 's/^.*MACC_TASK_RESULT:[[:space:]]*//')"
  raw="$(printf '%s' "$raw" | tr '[:upper:]' '[:lower:]' | tr '-' '_' | tr -d '\r' | xargs)"
  case "$raw" in
    success_with_changes) printf '%s' "success_with_changes" ;;
    success_without_changes) printf '%s' "success_without_changes" ;;
    already_satisfied|already_done|noop_success) printf '%s' "already_satisfied" ;;
    *) printf '%s' "" ;;
  esac
}

has_committable_changes() {
  if git status --porcelain | awk 'NF' | grep -q .; then
    if git status --porcelain | grep -vE '^[ MARCUD?!]{1,2} (performer\\.sh|worktree\\.prd\\.json)$' | awk 'NF' | grep -q .; then
      return 0
    fi
  fi
  return 1
}

detect_success_result_kind() {
  local output_file="$1"
  local explicit=""
  explicit="$(extract_task_result_marker "$output_file")"
  if [[ -n "$explicit" ]]; then
    printf '%s' "$explicit"
    return 0
  fi
  if has_committable_changes; then
    printf '%s' "success_with_changes"
  else
    printf '%s' "success_without_changes"
  fi
}

# RL-PERFORMER-009: classify E601/E602 from combined runner output.
# Sets LAST_ERROR_CODE, LAST_ERROR_ORIGIN, LAST_ERROR_MESSAGE.
# E602 is checked first (higher specificity — quota patterns are more specific).
detect_rate_limit() {
  local output_file="$1"
  [[ -f "$output_file" ]] || return 0
  local combined
  combined="$(cat "$output_file" 2>/dev/null | tr '[:upper:]' '[:lower:]')"
  # E602: hard quota exhaustion — do NOT retry
  if echo "$combined" | grep -qE \
      'quota[[:space:]]+exceeded|insufficient[_[:space:]]quota|usage[[:space:]]+limit[[:space:]]+reached|hit[[:space:]]+your[[:space:]]+limit|billing[[:space:]]+quota'; then
    LAST_ERROR_CODE="E602"
    LAST_ERROR_ORIGIN="runner"
    LAST_ERROR_MESSAGE="quota exhausted"
    return 0
  fi
  # E601: transient rate-limit / 429
  if echo "$combined" | grep -qE \
      '429|resource_exhausted|rate[[:space:]]+limit(ed)?|too[[:space:]]+many[[:space:]]+requests|529|overloaded'; then
    LAST_ERROR_CODE="E601"
    LAST_ERROR_ORIGIN="runner"
    LAST_ERROR_MESSAGE="rate limited"
    local retry_after
    retry_after="$(grep -iE 'retry.after:[[:space:]]*[0-9]+' "$output_file" 2>/dev/null \
        | grep -oE '[0-9]+' | tail -n1)"
    [[ -n "$retry_after" ]] && LAST_ERROR_MESSAGE="rate limited; retry-after=${retry_after}s"
    return 0
  fi
}

run_tool() {
  local prompt_file="$1"
  local attempt="$2"
  local max_attempts="$3"
  local output_capture
  local script
  script="$(tool_runner_path)"
  if [[ -z "$script" || ! -x "$script" ]]; then
    set_last_error "E102" "performer" "tool runner not found or not executable"
    echo "Error: tool performer not found or not executable: ${script}" >&2
    return 1
  fi
  output_capture="$(mktemp)"

  log_task_line "## Attempt ${attempt}/${max_attempts}"
  log_task_line ""
  log_task_line "- Runner: \`${script}\`"
  log_task_line "- Started: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  log_task_line ""
  log_task_line '```text'
  set +e
  emit_performer_event "progress" "$CURRENT_PHASE" "running" "$(jq -nc --arg attempt "$attempt" --arg max "$max_attempts" '{attempt:($attempt|tonumber?), max_attempts:($max|tonumber?)}')"
  spinner_start "Running ${tool} (attempt ${attempt}/${max_attempts})"
  "$script" \
    --prompt-file "$prompt_file" \
    --tool-json "$tool_json" \
    --repo "$repo" \
    --worktree "$worktree" \
    --task-id "$task_id" \
    --attempt "$attempt" \
    --max-attempts "$max_attempts" 2>&1 | tee "$output_capture" >>"$task_log_file"
  local status=${PIPESTATUS[0]}
  spinner_stop "Runner finished (${tool})"
  set -e

  if [[ "$status" -eq 0 ]]; then
    local result_kind=""
    local changed="false"
    result_kind="$(detect_success_result_kind "$output_capture")"
    if [[ "$result_kind" == "success_with_changes" ]]; then
      changed="true"
    fi
    must_emit_performer_event "phase_result" "$CURRENT_PHASE" "done" "$(jq -nc --arg attempt "$attempt" --arg result_kind "$result_kind" --argjson changed "$changed" '{
      attempt:($attempt|tonumber?),
      result_kind:($result_kind|select(length>0)),
      changed:$changed,
      message:(if $result_kind == "already_satisfied" then "Task already satisfied; verified with no code changes required."
               elif $result_kind == "success_without_changes" then "Task completed successfully with no repository changes."
               else "Task completed successfully with repository changes."
               end)
    }')"
    log_task_line "- Result kind: ${result_kind}"
  else
    # RL-PERFORMER-009: classify rate-limit signals before falling back to E101.
    detect_rate_limit "$output_capture"
    if [[ -z "$LAST_ERROR_CODE" || "$LAST_ERROR_CODE" == "E101" ]]; then
      set_last_error "E101" "runner" "runner exited non-zero"
    fi
    must_emit_performer_event "phase_result" "$CURRENT_PHASE" "failed" "$(jq -nc --arg attempt "$attempt" --arg status "$status" --arg code "$LAST_ERROR_CODE" --arg origin "$LAST_ERROR_ORIGIN" --arg message "$LAST_ERROR_MESSAGE" '{attempt:($attempt|tonumber?), exit_status:($status|tonumber?), error_code:($code|select(length>0)), origin:($origin|select(length>0)), message:($message|select(length>0))}')"
  fi
  log_task_line '```'
  log_task_line ""
  log_task_line "- Exit status: ${status}"
  log_task_line ""
  rm -f "$output_capture"
  return "$status"
}

commit_changes() {
  local last_id="$1"
  local last_title="$2"

  if git status --porcelain | awk 'NF' | grep -q .; then
    local git_add_output=""
    local git_commit_output=""
    # Stage everything first; protected files are un-staged right after.
    if ! git_add_output="$(git add -A 2>&1)"; then
      git_add_output="${git_add_output//$'\n'/ }"
      set_last_error "E202" "git" "git add failed: ${git_add_output:0:240}"
      return 1
    fi
    git reset -q HEAD -- performer.sh worktree.prd.json >/dev/null 2>&1 || true
    if git diff --cached --quiet; then
      if git status --porcelain -- performer.sh worktree.prd.json | awk 'NF' | grep -q .; then
        echo "No committable changes (protected files excluded: performer.sh, worktree.prd.json)."
      else
        echo "No changes to commit."
      fi
      return 0
    fi
    # --- MACC commit message convention (see core/src/commit_message.rs) ---
    # Subject: <type>: <task_id>[ - <title>]
    # Trailers: [macc:task <id>] [macc:phase <phase>]
    local subject="feat: ${last_id}"
    if [[ -n "$last_title" ]]; then
      subject="feat: ${last_id} - ${last_title}"
    fi
    local trailer="[macc:task ${last_id}]"
    if [[ -n "$CURRENT_PHASE" ]]; then
      trailer="${trailer}
[macc:phase ${CURRENT_PHASE}]"
    fi
    if [[ -n "$tool" ]]; then
      trailer="${trailer}
[macc:tool ${tool}]"
    fi
    if ! git_commit_output="$(git commit -m "$subject" -m "" -m "$trailer" 2>&1)"; then
      git_commit_output="${git_commit_output//$'\n'/ }"
      set_last_error "E201" "git" "git commit failed: ${git_commit_output:0:240}"
      return 1
    fi
    printf '%s\n' "$git_commit_output"
    local sha
    sha="$(git rev-parse HEAD 2>/dev/null || true)"
    local msg="${subject}"
    soft_emit_performer_event "commit_created" "$CURRENT_PHASE" "done" "$(jq -nc --arg sha "$sha" --arg message "$msg" '{sha:$sha, message:$message}')"
    echo "Committed changes: $subject"
  else
    echo "No changes to commit."
  fi
}

last_id=""
last_title=""
task_log_file="$(task_log_path "$task_id")"
log_task_header_if_needed "$task_log_file" "$task_id" "$task_id"
log_task_line "## Performer session"
log_task_line ""
log_task_line "- Task ID: ${task_id}"
log_task_line "- Coordinator IPC address: $(ipc_addr_display)"
log_task_line "- Started: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
log_task_line ""
soft_emit_performer_event "started" "$CURRENT_PHASE" "started" "$(jq -nc --arg tool "$tool" --arg worktree "$worktree" '{tool:$tool, worktree:$worktree}')"
heartbeat_start

for ((i=1; i<=PERFORMER_MAX_ITERATIONS; i++)); do
  next_task_json="$(get_next_task_json)"
  if [[ -z "$next_task_json" ]]; then
    commit_changes "$last_id" "$last_title"
    must_emit_performer_event "phase_result" "$CURRENT_PHASE" "done" "$(jq -nc '{
      attempt: 0,
      result_kind: "already_satisfied",
      changed: false,
      message: "Task already satisfied; no pending work remained in the worktree PRD."
    }')"
    TERMINAL_EVENT_EMITTED="true"
    exit 0
  fi

  next_id="$(get_next_task_id)"
  next_title="$(get_next_task_title)"
  task_log_file="$(task_log_path "$next_id")"
  log_task_header_if_needed "$task_log_file" "$next_id" "$next_title"
  log_task_line "## Processing task ${next_id}"
  log_task_line ""
  log_task_line "- Title: ${next_title}"
  log_task_line "- Started: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  log_task_line ""
  echo "Performer: task ${next_id} (${tool})"
  soft_emit_performer_event "progress" "$CURRENT_PHASE" "running" "$(jq -nc --arg task "$next_id" --arg title "$next_title" '{task_id:$task, title:$title}')"

  prompt_file="$(mktemp)"
  build_prompt "$next_task_json" "$next_id" "$next_title" >"$prompt_file"
  log_task_line "### Prompt"
  log_task_line ""
  log_task_line '```text'
  cat "$prompt_file" >>"$task_log_file"
  log_task_line '```'
  log_task_line ""

  tool_success=false
  for ((attempt=1; attempt<=PERFORMER_TOOL_MAX_ATTEMPTS; attempt++)); do
    if run_tool "$prompt_file" "$attempt" "$PERFORMER_TOOL_MAX_ATTEMPTS"; then
      tool_success=true
      break
    else
      attempt_rc=$?
      echo "Tool failed for task ${next_id} (attempt ${attempt}/${PERFORMER_TOOL_MAX_ATTEMPTS})" >&2
    fi
  done
  if [[ "$tool_success" != "true" ]]; then
    rm -f "$prompt_file"
    if [[ -z "$LAST_ERROR_CODE" ]]; then
      set_last_error "E101" "runner" "tool execution failed"
    fi
    must_emit_performer_event "failed" "$CURRENT_PHASE" "failed" "$(jq -nc --arg task "$next_id" --arg code "$LAST_ERROR_CODE" --arg origin "$LAST_ERROR_ORIGIN" --arg message "$LAST_ERROR_MESSAGE" '{task_id:$task, reason:"tool execution failed", error_code:($code|select(length>0)), origin:($origin|select(length>0)), message:($message|select(length>0))}')"
    TERMINAL_EVENT_EMITTED="true"
    echo "Error: tool execution failed for task ${next_id}" >&2
    exit 1
  fi
  rm -f "$prompt_file"

  mark_task_passed "$next_id"
  log_task_line "- Marked as passed in worktree PRD: ${next_id}"
  log_task_line "- Completed: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  log_task_line ""

  last_id="$next_id"
  last_title="$next_title"

  if [[ "$(pending_task_count)" -eq 0 ]]; then
    commit_changes "$last_id" "$last_title"
    TERMINAL_EVENT_EMITTED="true"
    exit 0
  fi

  sleep "$PERFORMER_SLEEP_SECONDS"
done

echo "Error: max iterations reached (${PERFORMER_MAX_ITERATIONS})" >&2
exit 1
