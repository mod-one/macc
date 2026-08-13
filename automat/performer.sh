#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  performer.sh --repo <path> --worktree <path> --task-id <id> --tool <tool> --registry <path> --prd <path>
               [--resume-attempt <n>] [--base-ref <ref>]

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
# Continuation support: when the coordinator re-dispatches a task that was
# parked after reporting `error_with_changes`, it passes the attempt number and
# the base ref so the prompt can describe what was already committed instead of
# asking the tool to start over on top of its own half-finished work.
resume_attempt="${MACC_RESUME_ATTEMPT:-0}"
base_ref="${MACC_BASE_REF:-}"
performer_log_dir=""
task_log_file=""
EVENT_FILE="${COORD_EVENTS_FILE:-}"
EVENT_IPC_ADDR="${MACC_COORDINATOR_IPC_ADDR:-}"
# Path to the coordinator's well-known IPC address file.  Used for
# coordinator-restart reconnection: when IPC fails, performer re-reads
# this file and retries with the new address.
EVENT_IPC_ADDR_FILE="${MACC_COORDINATOR_IPC_ADDR_FILE:-}"
EVENT_SOURCE="${MACC_EVENT_SOURCE:-}"
EVENT_TASK_ID="${MACC_EVENT_TASK_ID:-}"
EVENT_RUN_ID="${COORDINATOR_RUN_ID:-$(date +%s%N)-$$}"
EVENT_COORDINATOR_EPOCH="${COORDINATOR_EPOCH:-0}"
if [[ ! "$EVENT_COORDINATOR_EPOCH" =~ ^[0-9]+$ ]]; then
  EVENT_COORDINATOR_EPOCH=0
fi
EVENT_CLAIM_ID="${MACC_CLAIM_ID:-}"
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
    --resume-attempt) resume_attempt="$2"; shift 2 ;;
    --base-ref) base_ref="$2"; shift 2 ;;
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

# log_debug_line: write to task log only when MACC_DEBUG=1 is set.
# Use this for verbose entries that add noise in normal operation:
#   - full prompt dump (### Prompt)
#   - runner invocation line (- Runner:)
#   - attempt headers (## Attempt N/M)
log_debug_line() {
  if [[ "${MACC_DEBUG:-0}" == "1" ]]; then
    log_task_line "$@"
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
    --argjson coordinator_epoch "$EVENT_COORDINATOR_EPOCH" \
    --arg claim_id "$EVENT_CLAIM_ID" \
    --argjson seq "$seq" \
    --arg ts "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" \
    --arg source "$EVENT_SOURCE" \
    --arg task_id "$EVENT_TASK_ID" \
    --arg type "$event_type" \
    --arg phase "$phase" \
    --arg status "$status" \
    --argjson payload "$payload_json" \
    '({
      schema_version:$schema_version,
      event_id:$event_id,
      run_id:$run_id,
      coordinator_epoch:$coordinator_epoch,
      seq:$seq,
      ts:$ts,
      source:$source,
      task_id:$task_id,
      type:$type,
      status:$status,
      payload:$payload
    }
    + (if $claim_id != "" then {claim_id:$claim_id} else {} end)
    + (if $phase    != "" then {phase:$phase}       else {} end))' 2>"$jq_err_file")"; then
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
  # IPC failed with the baked-in address. If the coordinator was restarted
  # (e.g., after SSH disconnect), the address file may have a new address.
  # Read it and retry once so performers survive coordinator restarts.
  if [[ -n "$EVENT_IPC_ADDR_FILE" && -f "$EVENT_IPC_ADDR_FILE" ]]; then
    local new_addr
    new_addr="$(< "$EVENT_IPC_ADDR_FILE" tr -d '[:space:]')"
    if [[ -n "$new_addr" && "$new_addr" != "$EVENT_IPC_ADDR" ]]; then
      local old_addr="$EVENT_IPC_ADDR"
      EVENT_IPC_ADDR="$new_addr"
      if send_event_via_ipc "$event_line"; then
        # Successfully reconnected to restarted coordinator.
        return 0
      fi
      # Retry with new addr also failed — restore original for error context.
      EVENT_IPC_ADDR="$old_addr"
    fi
  fi
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
    '({
      exit_code:($exit|tonumber?)
    }
    + (if $code   != "" then {error_code:$code} else {} end)
    + (if $origin != "" then {origin:$origin}   else {} end)
    + (if $msg    != "" then {message:$msg}     else {} end))'
}

heartbeat_start() {
  local tool_runner_pid="${1:-}"
  [[ -n "$EVENT_FILE" || -n "$EVENT_IPC_ADDR" ]] || return 0
  heartbeat_stop
  (
    while true; do
      # When tracking a tool runner PID, verify it is still alive.
      # If the runner exited but the performer shell has not yet reaped it,
      # emit a final "stale" heartbeat and stop — this lets the coordinator's
      # stale-heartbeat policy detect the zombie state promptly.
      if [[ -n "$tool_runner_pid" ]] && ! kill -0 "$tool_runner_pid" 2>/dev/null; then
        soft_emit_performer_event "heartbeat" "$CURRENT_PHASE" "stale" '{"reason":"tool_runner_exited"}'
        break
      fi
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

# Read the explanation this task recorded on its previous attempt.
#
# The performer's own task log is the authoritative local record: it holds both
# the raw `MACC_TASK_RESULT_EXP:` line and the normalised `- Explanation:` line
# it writes after each attempt. Reading it here keeps the continuation prompt
# self-contained -- no dependency on coordinator state round-tripping back.
previous_result_explanation() {
  local id="$1"
  local path
  path="$(task_log_path "$id")"
  [[ -f "$path" ]] || return 0
  grep -E '^- Explanation:' "$path" | tail -n 1 | sed -E 's/^- Explanation:[[:space:]]*//'
}

# Summarise what the previous attempt already committed on this branch.
build_prior_work_summary() {
  local base="$1"
  [[ -n "$base" ]] || return 0
  git rev-parse --verify "$base" >/dev/null 2>&1 || return 0

  local commits stat
  commits="$(git log --oneline "${base}..HEAD" 2>/dev/null || true)"
  [[ -n "$commits" ]] || return 0
  stat="$(git diff --stat "${base}..HEAD" 2>/dev/null | tail -n 40 || true)"

  printf 'Commits already made on this branch (not yet merged into %s):\n\n%s\n\nFiles changed:\n\n%s\n' \
    "$base" "$commits" "$stat"
}

build_continuation_section() {
  local task_id="$1"
  local prior_exp prior_work
  prior_exp="$(previous_result_explanation "$task_id")"
  prior_work="$(build_prior_work_summary "$base_ref")"

  cat <<CONT

## CONTINUATION — this task was already started

This is attempt ${resume_attempt} of this task. A previous attempt implemented
part of it, committed that work, and then reported that it could not finish.
The commits below are already on this branch and are YOURS -- they are not
someone else's work and they are not merged yet.

Reason the previous attempt stopped:
${prior_exp:-(not recorded)}

${prior_work:-(no commits found on this branch; treat the task as unstarted)}

Note: the base branch (${base_ref:-unknown}) may have advanced since those
commits were made, so files you touched may look different from what you left.

How to proceed:
1) FIRST assess the current state: read the committed work above and check what
   the task still requires. Do not re-derive the implementation from scratch.
2) Then complete ONLY the remaining work. Do not revert, rewrite, or duplicate
   what is already committed and correct.
3) If the remaining gap is a pre-existing repository problem outside this
   task's scope (for example an unrelated failing test or build target), the
   task is DONE: report success and state the out-of-scope issue in your
   explanation. Do not report an error for problems this task did not cause.
4) If the previously committed work is genuinely unusable and must be discarded,
   do NOT quietly rewrite it -- stop and report:
   MACC_TASK_RESULT_EXP: prior work unsalvageable: <reason>
   MACC_TASK_RESULT: error_without_changes
   so the coordinator can reset the branch and restart the task cleanly.
CONT
}

build_prompt() {
  local task_json="$1"
  local task_id="$2"
  local task_title="$3"
  local continuation=""
  local prompt_closing_line="Now implement the task !"
  if [[ "${resume_attempt:-0}" =~ ^[0-9]+$ ]] && (( resume_attempt > 0 )); then
    continuation="$(build_continuation_section "$task_id")"
    prompt_closing_line="Now assess the committed work above, then finish the remaining task !"
  fi
  cat <<PROMPT

Context:
- Worktree: ${worktree}
- Task ID: ${task_id}
- Task Title: ${task_title}

Task (JSON):
${task_json}
${continuation}

Instructions:
1) Implement ONLY the task above.
2) Do NOT edit or read ${prd}; the runner will update it.
3) Do NOT commit; the runner will commit if all tasks are done.
4) Keep output concise; avoid dumping large files.
5) Use concise professional fragments by default.
6) Avoid explaining code.
7) Avoid repeated task restatements
8) Avoid broad educational explanations
9) If the task acceptance criteria are already satisfied before any code change, this is a valid success. Verify it explicitly and do not make unnecessary edits.
10) At the end, print exactly one terminal result marker on its own line:
   - MACC_TASK_RESULT: success_with_changes
   - MACC_TASK_RESULT: success_without_changes
   - MACC_TASK_RESULT: already_satisfied
   - MACC_TASK_RESULT: error_with_changes   (if you started work but cannot finish)
   - MACC_TASK_RESULT: error_without_changes (if you could not start or make any progress)
11) Use already_satisfied only when you verified the task is already done and can cite the evidence briefly.
12) Use error_with_changes or error_without_changes ONLY when THIS task could not be completed (sandbox failures, environment issues, missing dependencies, permission errors, etc.). Include a brief explanation of why on the line before the marker. The explanation must start with "MACC_TASK_RESULT_EXP:".
13) Pre-existing repository problems that this task did not cause and is not scoped to fix are NOT a reason to report an error. If a repo-wide check (test suite, build, lint) fails only in areas unrelated to this task, and this task's own work is complete and verified, report success and note the unrelated failures in your explanation. Judge this task by its own acceptance criteria, not by the health of the whole repository.
14) If you finish successfully but forget the marker, the runner will infer the result from repository state; still print the marker explicitly.

${prompt_closing_line}
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
    error_with_changes) printf '%s' "error_with_changes" ;;
    error_without_changes|error|failed) printf '%s' "error_without_changes" ;;
    *) printf '%s' "" ;;
  esac
}

extract_task_result_exp() {
  local output_file="$1"
  local raw=""
  raw="$(grep -E 'MACC_TASK_RESULT_EXP:' "$output_file" | tail -n 1 | sed -E 's/^.*MACC_TASK_RESULT_EXP:[[:space:]]*//')"
  printf '%s' "$raw" | tr -d '\r' | xargs
}

# A terminal `error_*` result without an explanation leaves the coordinator --
# and the operator, and any later continuation attempt -- with no record of why
# the tool stopped. The prompt requires `MACC_TASK_RESULT_EXP:`; when the tool
# omits it anyway, substitute an explicit placeholder rather than propagating an
# empty string, so the gap is visible instead of silent.
resolve_task_result_exp() {
  local output_file="$1"
  local result_kind="$2"
  local exp=""
  exp="$(extract_task_result_exp "$output_file")"
  if [[ -z "$exp" && "$result_kind" == error_* ]]; then
    exp="(no explanation provided: the tool reported ${result_kind} without the required MACC_TASK_RESULT_EXP line)"
    echo "Warning: ${result_kind} reported without MACC_TASK_RESULT_EXP" >&2
    log_task_line "- Warning: ${result_kind} reported without MACC_TASK_RESULT_EXP"
  fi
  printf '%s' "$exp"
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
      '429|resource_exhausted|model_capacity_exhausted|no[[:space:]]+capacity[[:space:]]+available|rate[[:space:]]+limit(ed)?|too[[:space:]]+many[[:space:]]+requests|529|overloaded'; then
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

  log_debug_line "## Attempt ${attempt}/${max_attempts}"
  log_debug_line ""
  log_debug_line "- Runner: \`${script}\`"
  log_debug_line "- Started: $(date -u +"%Y-%m-%dT%H:%M:%SZ")"
  log_debug_line ""
  set +e
  emit_performer_event "progress" "$CURRENT_PHASE" "running" "$(jq -nc --arg attempt "$attempt" --arg max "$max_attempts" '{attempt:($attempt|tonumber?), max_attempts:($max|tonumber?)}')"
  spinner_start "Running ${tool} (attempt ${attempt}/${max_attempts})"

  # Run the tool runner in background so we can capture its PID and let the
  # heartbeat loop verify liveness.  Output is written to the capture file
  # and appended to the task log after the runner exits.
  local session_args=()
  if [[ -n "$performer_session_id" ]]; then
    session_args=("--session-id" "$performer_session_id")
  fi
  "$script" \
    --prompt-file "$prompt_file" \
    --tool-json "$tool_json" \
    --repo "$repo" \
    --worktree "$worktree" \
    --task-id "$task_id" \
    --attempt "$attempt" \
    --max-attempts "$max_attempts" \
    "${session_args[@]}" >"$output_capture" 2>&1 &
  local runner_pid=$!

  # Restart heartbeat with runner PID tracking — stops emitting if the
  # runner exits unexpectedly, so stale-heartbeat detection works.
  heartbeat_stop
  heartbeat_start "$runner_pid"

  wait "$runner_pid"
  local status=$?

  # Append captured output to the task log (replaces the previous tee pipe).
  cat "$output_capture" >>"$task_log_file" 2>/dev/null

  # Restore plain heartbeat (no PID tracking) between tasks.
  heartbeat_stop
  heartbeat_start

  spinner_stop "Runner finished (${tool})"
  set -e

  if [[ "$status" -eq 0 ]]; then
    local result_kind=""
    local changed="false"
    result_kind="$(detect_success_result_kind "$output_capture")"
    if [[ "$result_kind" == "success_with_changes" ]]; then
      changed="true"
    fi
    local result_exp=""
    result_exp="$(resolve_task_result_exp "$output_capture" "$result_kind")"
    # Build the payload additively: optional string fields are merged in only
    # when non-empty. Do NOT use `field:($x|select(length>0))` inside the object
    # literal — jq's `select` yields `empty` for an empty string, and a field
    # whose value is `empty` collapses the ENTIRE object to no output. That is
    # what silently produced a `{}` payload (missing result_kind) and caused the
    # coordinator to reject successful terminal events.
    # Fatal on rejection: `run_tool` is invoked as the condition of an `if`
    # statement by its caller (`if run_tool ...; then`), which means `set -e`
    # is suspended for this entire function body -- a non-zero return from
    # must_emit_performer_event here would NOT abort the script on its own,
    # and `run_tool` would still `return "$status"` (the tool's own exit code,
    # unrelated to whether the terminal event was accepted). This previously
    # let the performer mark the task passed and exit 0 even though the
    # coordinator had rejected the only record of that success. An explicit
    # `exit 1` is required to terminate the script regardless of call context.
    if ! must_emit_performer_event "phase_result" "$CURRENT_PHASE" "done" "$(jq -nc \
      --arg attempt "$attempt" \
      --arg result_kind "$result_kind" \
      --argjson changed "$changed" \
      --arg result_exp "$result_exp" \
      '({
        attempt:($attempt|tonumber?),
        changed:$changed,
        message:(if $result_kind == "already_satisfied" then "Task already satisfied; verified with no code changes required."
                 elif $result_kind == "success_without_changes" then "Task completed successfully with no repository changes."
                 elif $result_kind == "error_with_changes" then "Tool execution failed with repository changes."
                 elif $result_kind == "error_without_changes" then "Tool execution failed with no repository changes."
                 else "Task completed successfully with repository changes."
                 end)
      }
      + (if $result_kind != "" then {result_kind:$result_kind} else {} end)
      + (if $result_exp  != "" then {result_exp:$result_exp}  else {} end))')"; then
      echo "Error: failed to persist terminal phase_result event (status=done); refusing to mark task passed" >&2
      log_task_line "- Exit status: ${status}"
      exit 1
    fi
    log_task_line "- Result kind: ${result_kind}"
    if [[ -n "$result_exp" ]]; then
      log_task_line "- Explanation: ${result_exp}"
    fi
  else
    # RL-PERFORMER-009: classify rate-limit signals before falling back to E101.
    detect_rate_limit "$output_capture"
    if [[ -z "$LAST_ERROR_CODE" || "$LAST_ERROR_CODE" == "E101" ]]; then
      set_last_error "E101" "runner" "runner exited non-zero"
    fi
    local result_exp=""
    result_exp="$(resolve_task_result_exp "$output_capture" "error_runner_exit")"
    # Same additive-merge pattern as the success path above: never let an empty
    # optional field collapse the whole object (which would drop error_code and
    # cause the failure to be misclassified).
    #
    # Not made fatal-on-rejection like the "done" path: this is a per-attempt
    # progress signal, not the mandatory terminal record. run_tool() always
    # returns the runner's own non-zero $status regardless of whether this
    # event persists, so the retry loop and the final unconditional exit 1
    # (after max attempts) already guarantee a correct non-zero exit without
    # aborting mid-retry over a possibly-transient IPC hiccup.
    must_emit_performer_event "phase_result" "$CURRENT_PHASE" "failed" "$(jq -nc \
      --arg attempt "$attempt" \
      --arg status "$status" \
      --arg code "$LAST_ERROR_CODE" \
      --arg origin "$LAST_ERROR_ORIGIN" \
      --arg message "$LAST_ERROR_MESSAGE" \
      --arg result_exp "$result_exp" \
      '({
        attempt:($attempt|tonumber?),
        exit_status:($status|tonumber?)
      }
      + (if $code       != "" then {error_code:$code}       else {} end)
      + (if $origin     != "" then {origin:$origin}         else {} end)
      + (if $message    != "" then {message:$message}       else {} end)
      + (if $result_exp != "" then {result_exp:$result_exp} else {} end))')"
  fi
  log_task_line ""
  log_task_line "- Exit status: ${status}"
  log_task_line ""
  rm -f "$output_capture"

  # Refresh session ID from tool-sessions.json so subsequent iterations and
  # attempts reuse the session established by the tool runner.
  if [[ "$performer_session_enabled" == "true" ]]; then
    local new_sid
    new_sid="$(performer_read_session_id)"
    if [[ -n "$new_sid" && "$new_sid" != "$performer_session_id" ]]; then
      performer_session_id="$new_sid"
      log_task_line "- Session ID updated: ${performer_session_id}"
    elif [[ -z "$performer_session_id" && -n "$new_sid" ]]; then
      performer_session_id="$new_sid"
      log_task_line "- Session ID acquired: ${performer_session_id}"
    fi
  fi

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

# ---------------------------------------------------------------------------
# Session tracking — the coordinator performer reads and passes session IDs
# to the tool runner, just like the tool runner itself manages sessions for
# the underlying tool.
# ---------------------------------------------------------------------------
performer_session_id=""
performer_session_state_file="${repo}/.macc/state/tool-sessions.json"
performer_session_enabled="$(jq -r '.performer.session.enabled // false' "$tool_json")"
performer_session_scope="$(jq -r '.performer.session.scope // "worktree"' "$tool_json")"
performer_session_id_strategy="$(jq -r '.performer.session.id_strategy // "discovered"' "$tool_json")"

performer_session_key() {
  if [[ "$performer_session_scope" == "project" ]]; then
    echo "project"
  else
    echo "$worktree"
  fi
}

# Read the current session ID from tool-sessions.json for this worktree/tool.
performer_read_session_id() {
  local key
  key="$(performer_session_key)"
  [[ -f "$performer_session_state_file" ]] || { echo ""; return 0; }
  jq -r --arg tool "$tool" --arg key "$key" '
    .tools[$tool].sessions[$key].session_id // empty
  ' "$performer_session_state_file" 2>/dev/null || echo ""
}

# Initialise performer_session_id from tool-sessions.json.
if [[ "$performer_session_enabled" == "true" ]]; then
  performer_session_id="$(performer_read_session_id)"
  if [[ -n "$performer_session_id" ]]; then
    echo "Performer: reusing session ${performer_session_id}" >&2
  fi
fi

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
    # Fatal on rejection (see the "done" path in run_tool() for the full
    # rationale): do not set TERMINAL_EVENT_EMITTED or exit 0 unless the
    # coordinator actually accepted this terminal event. Leaving
    # TERMINAL_EVENT_EMITTED unset on failure lets the on_exit trap's
    # synthetic "failed" event fire as a fallback signal.
    if ! must_emit_performer_event "phase_result" "$CURRENT_PHASE" "done" "$(jq -nc '{
      attempt: 0,
      result_kind: "already_satisfied",
      changed: false,
      message: "Task already satisfied; no pending work remained in the worktree PRD."
    }')"; then
      echo "Error: failed to persist terminal phase_result event (status=already_satisfied)" >&2
      exit 1
    fi
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
  if [[ "${MACC_DEBUG:-0}" == "1" ]]; then
    log_task_line "### Prompt"
    log_task_line '---'
    log_task_line ""
    cat "$prompt_file" >>"$task_log_file"
    log_task_line '---'
    log_task_line ""
  fi

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
    must_emit_performer_event "failed" "$CURRENT_PHASE" "failed" "$(jq -nc --arg task "$next_id" --arg code "$LAST_ERROR_CODE" --arg origin "$LAST_ERROR_ORIGIN" --arg message "$LAST_ERROR_MESSAGE" '({task_id:$task, reason:"tool execution failed"} + (if $code != "" then {error_code:$code} else {} end) + (if $origin != "" then {origin:$origin} else {} end) + (if $message != "" then {message:$message} else {} end))')"
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
