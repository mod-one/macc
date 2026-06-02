#!/usr/bin/env bash
set -euo pipefail

# Shared performer logic extracted from adapter-specific performer scripts.
# Required before sourcing:
#   TOOL_ID         - adapter id for usage text
#   TOOL_LOG_PREFIX - log prefix for invoke messages
: "${TOOL_ID:?TOOL_ID must be set before sourcing adapters/shared/performer_lib.sh}"
: "${TOOL_LOG_PREFIX:?TOOL_LOG_PREFIX must be set before sourcing adapters/shared/performer_lib.sh}"

usage() {
  cat <<'EOF'
Usage:
  ${TOOL_ID}.performer.sh --prompt-file <path> --tool-json <path> [--repo <path>] [--worktree <path>] [--task-id <id>] [--attempt N] [--max-attempts N]
EOF
}

prompt_file=""
tool_json=""
repo=""
worktree=""
task_id=""
attempt="1"
max_attempts="1"
caller_session_id=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prompt-file) prompt_file="$2"; shift 2 ;;
    --tool-json) tool_json="$2"; shift 2 ;;
    --repo) repo="$2"; shift 2 ;;
    --worktree) worktree="$2"; shift 2 ;;
    --task-id) task_id="$2"; shift 2 ;;
    --attempt) attempt="$2"; shift 2 ;;
    --max-attempts) max_attempts="$2"; shift 2 ;;
    --session-id) caller_session_id="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 1 ;;
  esac
done

if [[ -z "$prompt_file" || ! -f "$prompt_file" ]]; then
  echo "Error: prompt file missing: $prompt_file" >&2
  exit 1
fi

if [[ -z "$tool_json" ]]; then
  tool_json=".macc/tool.json"
fi
if [[ ! -f "$tool_json" ]]; then
  echo "Error: tool.json missing: $tool_json" >&2
  exit 1
fi

if [[ -z "$repo" ]]; then
  repo="$(pwd)"
fi
if [[ -z "$worktree" ]]; then
  worktree="$(pwd)"
fi

command="$(jq -r '.performer.command // empty' "$tool_json")"
if [[ -z "$command" ]]; then
  echo "Error: performer.command missing in tool.json" >&2
  exit 1
fi
if ! command -v "$command" >/dev/null 2>&1; then
  echo "Error: tool command not found in PATH: $command" >&2
  exit 1
fi

tool_id="$(jq -r '.id // empty' "$tool_json")"
if [[ -z "$tool_id" || "$tool_id" == "null" ]]; then
  tool_id="tool"
fi

session_enabled="$(jq -r '.performer.session.enabled // false' "$tool_json")"
session_scope="$(jq -r '.performer.session.scope // "worktree"' "$tool_json")"
session_init_prompt="$(jq -r '.performer.session.init_prompt // "Bonjour"' "$tool_json")"
session_extract_regex="$(jq -r '.performer.session.extract_regex // "session[[:space:]]+id:[[:space:]]*([[:alnum:]-]+)"' "$tool_json")"
session_resume_command="$(jq -r '.performer.session.resume.command // empty' "$tool_json")"
session_discover_command="$(jq -r '.performer.session.discover.command // empty' "$tool_json")"
session_id_strategy="$(jq -r '.performer.session.id_strategy // "discovered"' "$tool_json")"
session_state_file="${repo}/.macc/state/tool-sessions.json"
session_lock_dir="${session_state_file}.lock"
session_lease_ttl="${SESSION_LEASE_TTL_SECONDS:-1800}"
# Max age (seconds) for a session to be offered for resume. Sessions whose
# last_used_at is older than this are skipped; the tool will start fresh.
# Default 86400 s (24 h). Override via SESSION_MAX_AGE_SECONDS or tool.json.
session_max_age_seconds="${SESSION_MAX_AGE_SECONDS:-$(jq -r '.performer.session.max_age_seconds // 86400' "$tool_json")}"
# Maximum number of available (non-active) sessions kept in the pool per tool.
# Oldest entries are pruned when this cap is exceeded after a new session write.
# Default 8. Override via SESSION_POOL_CAP or tool.json.
session_pool_cap="${SESSION_POOL_CAP:-$(jq -r '.performer.session.pool_cap // 8' "$tool_json")}"
mkdir -p "$(dirname "$session_state_file")"

acquire_session_lock() {
  local attempts=0
  until mkdir "$session_lock_dir" 2>/dev/null; do
    attempts=$((attempts + 1))
    if [[ "$attempts" -ge 80 ]]; then
      echo "Error: timed out acquiring session lock: $session_lock_dir" >&2
      return 1
    fi
    sleep 0.1
  done
}

release_session_lock() {
  rmdir "$session_lock_dir" >/dev/null 2>&1 || true
}

now_iso() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

now_epoch() {
  date -u +%s
}

# Returns true (exit 0) when session $sid is actively held by another live
# process that has refreshed its heartbeat within session_lease_ttl.
session_occupied_by_other() {
  local sid="$1"
  local status hb now age
  [[ -n "$sid" ]] || return 1
  [[ -f "$session_state_file" ]] || return 1

  status="$(jq -r --arg tool "$tool_id" --arg sid "$sid" \
    '(.tools[$tool].sessions[$sid].status // "available")' \
    "$session_state_file" 2>/dev/null)"
  [[ "$status" == "active" ]] || return 1

  hb="$(jq -r --arg tool "$tool_id" --arg sid "$sid" \
    '(.tools[$tool].sessions[$sid].heartbeat_epoch // 0)' \
    "$session_state_file" 2>/dev/null)"
  [[ "$hb" =~ ^[0-9]+$ ]] || hb=0

  now="$(now_epoch)"
  age=$((now - hb))
  (( age <= session_lease_ttl )) && return 0
  return 1
}

# Scan the tool's session pool and return the first session that is not
# currently occupied and not too old to be worth resuming.
# Only considers new-format entries (keyed by session_id); old-format entries
# (keyed by worktree path, with a nested session_id field) are silently skipped.
# Sessions whose last use timestamp is older than session_max_age_seconds are
# also skipped — those are unlikely to be resumable by the tool.
find_available_session_id() {
  [[ -f "$session_state_file" ]] || { echo ""; return 0; }
  local now_ts
  now_ts="$(now_epoch)"
  local sids
  sids="$(jq -r --arg tool "$tool_id" \
      --argjson now "$now_ts" \
      --argjson max_age "$session_max_age_seconds" '
    (.tools[$tool].sessions // {}) | to_entries[] |
    select(
      (.value | type) == "object" and
      (.value.session_id == null) and
      (
        (.value.last_used_at // .value.updated_at // .value.created_at // "") as $ts |
        if $ts == "" then true
        else
          (try ($ts | strptime("%Y-%m-%dT%H:%M:%SZ") | mktime) catch -1) as $epoch |
          if $epoch < 0 then true
          else ($now - $epoch) <= $max_age
          end
        end
      )
    ) |
    .key
  ' "$session_state_file" 2>/dev/null)"
  local sid
  while IFS= read -r sid; do
    [[ -z "$sid" ]] && continue
    if ! session_occupied_by_other "$sid"; then
      echo "$sid"
      return 0
    fi
  done <<< "$sids"
  echo ""
}

# Write or update an active lease for session $sid.
# Optional second argument: creation_reason — recorded only when the session
# entry is brand-new (no existing entry). Preserved on subsequent writes.
# use_count is incremented on every acquisition.
write_active_lease() {
  local sid="$1"
  local creation_reason="${2:-new}"
  local now ts tmp
  now="$(now_iso)"
  ts="$(now_epoch)"
  tmp="$(mktemp)"

  if [[ -f "$session_state_file" ]]; then
    jq \
      --arg tool "$tool_id" \
      --arg sid "$sid" \
      --arg now "$now" \
      --arg wt "$worktree" \
      --arg tid "$task_id" \
      --arg pid "$$" \
      --arg reason "$creation_reason" \
      --argjson hb "$ts" '
      .tools = (.tools // {}) |
      .tools[$tool] = (.tools[$tool] // {}) |
      .tools[$tool].sessions = (.tools[$tool].sessions // {}) |
      .tools[$tool].sessions[$sid] = (
        (.tools[$tool].sessions[$sid] // {
          created_at: $now,
          creation_reason: $reason,
          use_count: 0
        }) |
        .use_count = ((.use_count // 0) + 1) |
        . + {
          status: "active",
          owner_worktree: $wt,
          owner_task_id: $tid,
          owner_pid: $pid,
          heartbeat_epoch: $hb,
          updated_at: $now
        }
      )
      ' "$session_state_file" >"$tmp"
  else
    jq -n \
      --arg tool "$tool_id" \
      --arg sid "$sid" \
      --arg now "$now" \
      --arg wt "$worktree" \
      --arg tid "$task_id" \
      --arg pid "$$" \
      --arg reason "$creation_reason" \
      --argjson hb "$ts" '{
        tools: {
          ($tool): {
            sessions: {
              ($sid): {
                status: "active",
                created_at: $now,
                creation_reason: $reason,
                use_count: 1,
                owner_worktree: $wt,
                owner_task_id: $tid,
                owner_pid: $pid,
                heartbeat_epoch: $hb,
                updated_at: $now
              }
            }
          }
        }
      }' >"$tmp"
  fi

  mv "$tmp" "$session_state_file"
}

# Remove the oldest available (non-active) sessions for this tool so the pool
# stays under $1 entries. Active sessions are never touched. Old-format entries
# (with a nested session_id field) are treated as non-prunable and kept.
prune_pool_available() {
  local cap="$1"
  [[ -f "$session_state_file" ]] || return 0
  local tmp
  tmp="$(mktemp)"
  jq --arg tool "$tool_id" --argjson cap "$cap" '
    .tools[$tool].sessions = (
      .tools[$tool].sessions // {} |
      to_entries |
      ( map(select(
          .value.session_id != null or
          (.value.status // "available") == "active"
        )) ) as $protected |
      ( map(select(
          .value.session_id == null and
          (.value.status // "available") != "active"
        )) |
        sort_by(.value.last_used_at // .value.updated_at // .value.created_at // "") |
        reverse |
        .[0:$cap] ) as $keep_avail |
      ($protected + $keep_avail) | from_entries
    )
  ' "$session_state_file" >"$tmp" && mv "$tmp" "$session_state_file" || rm -f "$tmp"
}

mark_lease_status() {
  local sid="$1"
  local status="$2"
  local now ts tmp
  [[ -n "$sid" ]] || return 0
  [[ -f "$session_state_file" ]] || return 0
  now="$(now_iso)"
  ts="$(now_epoch)"
  tmp="$(mktemp)"
  jq \
    --arg tool "$tool_id" \
    --arg sid "$sid" \
    --arg status "$status" \
    --arg now "$now" \
    --arg tid "${task_id:-}" \
    --argjson hb "$ts" '
    .tools = (.tools // {}) |
    .tools[$tool] = (.tools[$tool] // {}) |
    .tools[$tool].sessions = (.tools[$tool].sessions // {}) |
    if (.tools[$tool].sessions[$sid] // null) != null then
      .tools[$tool].sessions[$sid].status = $status |
      .tools[$tool].sessions[$sid].updated_at = $now |
      if $status == "active" then
        .tools[$tool].sessions[$sid].heartbeat_epoch = $hb
      else
        .tools[$tool].sessions[$sid].heartbeat_epoch = 0 |
        .tools[$tool].sessions[$sid].last_used_at = $now |
        (if $tid != "" then .tools[$tool].sessions[$sid].last_task_id = $tid else . end) |
        del(.tools[$tool].sessions[$sid].owner_worktree) |
        del(.tools[$tool].sessions[$sid].owner_task_id) |
        del(.tools[$tool].sessions[$sid].owner_pid)
      end
    else . end
    ' "$session_state_file" >"$tmp"
  mv "$tmp" "$session_state_file"
}

extract_session_id_from_output() {
  local output_file="$1"
  local regex="$2"
  local found=""
  shopt -s nocasematch
  while IFS= read -r line; do
    if [[ "$line" =~ $regex ]]; then
      found="${BASH_REMATCH[1]}"
    fi
  done <"$output_file"
  shopt -u nocasematch
  printf "%s" "$found"
}

run_and_capture() {
  local output_file="$1"
  shift
  local rc=0
  printf '[MACC] invoke (${TOOL_LOG_PREFIX} 1): %s\n' "$*" >&2
  "$@" 2>&1 | tee "$output_file"
  rc=${PIPESTATUS[0]}
  return "$rc"
}

expand_config_args() {
  local sid="$1"
  local out_name="$2"
  local -n out_ref="$out_name"
  local token=""
  local current_sid="${sid:-}"

  out_ref=()

  for token in "${args[@]}"; do
    out_ref+=("${token//\{session_id\}/$current_sid}")
  done
}

run_resume_and_capture() {
  local output_file="$1"
  local sid="$2"
  local prompt="$3"
  local final_args=()
  local arg
  local expanded_retry_args=()
  local i=0

  # 1. Base resume args from config
  while IFS= read -r arg; do
    final_args+=("${arg//\{session_id\}/$sid}")
  done < <(jq -r '.performer.session.resume.args[]?' "$tool_json")

  # 2. Inject attempt-specific flags from retry overrides.
  expand_config_args "$sid" expanded_retry_args
  while [[ $i -lt ${#expanded_retry_args[@]} ]]; do
    local a="${expanded_retry_args[$i]}"
    local substituted_a="$a"
    if [[ "$substituted_a" == -* ]]; then
      local next_idx=$((i + 1))
      if [[ " ${final_args[*]} " != *" ${substituted_a} "* ]]; then
        final_args+=("$substituted_a")
        if [[ $next_idx -lt ${#expanded_retry_args[@]} ]]; then
          local next_a="${expanded_retry_args[$next_idx]}"
          final_args+=("$next_a")
          i=$next_idx
        fi
      elif [[ $next_idx -lt ${#expanded_retry_args[@]} ]]; then
        i=$next_idx
      fi
    fi
    i=$((i + 1))
  done

  # Apply tier model override to resume args.
  # Strategy A (CLI flag) only — config file was already updated by _apply_tier_model_to_args.
  if [[ -n "$_tier_model" ]]; then
    local _r_prev_applied="$_tier_model_applied_via_args"
    _tier_model_applied_via_args=false
    _replace_model_in_args final_args
    if $_tier_model_applied_via_args && [[ -n "$_tier_effort" && -n "$_effort_flag" ]]; then
      local _r_has=false
      for _r_a in "${final_args[@]}"; do [[ "$_r_a" == "$_effort_flag" ]] && _r_has=true && break; done
      $_r_has || final_args+=("$_effort_flag" "$_tier_effort")
    fi
    _tier_model_applied_via_args="$_r_prev_applied"
  fi

  if [[ "$prompt_mode" == "arg" && -n "$prompt_arg" ]]; then
    run_and_capture "$output_file" "$session_resume_command" "${final_args[@]}" "$prompt_arg" "$prompt"
  else
    run_and_capture "$output_file" "$session_resume_command" "${final_args[@]}" "$prompt"
  fi
}

discover_session_id() {
  local output_file="$1"
  local discover_args=()
  local arg
  local sid=""
  local last_line=""

  if [[ -z "$session_discover_command" ]]; then
    echo ""
    return 0
  fi

  while IFS= read -r arg; do
    discover_args+=("$arg")
  done < <(jq -r '.performer.session.discover.args[]?' "$tool_json")

  run_and_capture "$output_file" "$session_discover_command" "${discover_args[@]}" >/dev/null || true
  sid="$(extract_session_id_from_output "$output_file" "$session_extract_regex")"
  if [[ -n "$sid" ]]; then
    echo "$sid"
    return 0
  fi

  last_line="$(awk 'NF{line=$0} END{print line}' "$output_file" | tr -d '\r')"
  echo "$last_line"
}

generate_session_id() {
  if command -v uuidgen >/dev/null 2>&1; then
    uuidgen | tr -d '\r'
    return 0
  fi
  if [[ -r /proc/sys/kernel/random/uuid ]]; then
    cat /proc/sys/kernel/random/uuid | tr -d '\r'
    return 0
  fi
  date -u +%Y%m%dT%H%M%S%N
}

reserve_generated_session_id() {
  local attempts=0
  local sid=""
  while [[ "$attempts" -lt 10 ]]; do
    sid="$(generate_session_id)"
    [[ -n "$sid" ]] || {
      attempts=$((attempts + 1))
      continue
    }
    if ! session_occupied_by_other "$sid"; then
      write_active_lease "$sid" "generated"
      active_session_id="$sid"
      printf "%s" "$sid"
      return 0
    fi
    attempts=$((attempts + 1))
  done
  return 1
}

args=()
if [[ "$attempt" -gt 1 ]] && jq -e '.performer.retry' "$tool_json" >/dev/null 2>&1; then
  command="$(jq -r '.performer.retry.command // .performer.command' "$tool_json")"
  while IFS= read -r arg; do
    args+=("$arg")
  done < <(jq -r '.performer.retry.args[]?' "$tool_json")
else
  while IFS= read -r arg; do
    args+=("$arg")
  done < <(jq -r '.performer.args[]?' "$tool_json")
fi

# ── Model tier routing (spec §8) ──────────────────────────────────────────────
# When MACC_MODEL_TIER is set (injected by coordinator model_routing.rs or via
# CLI --model-tier), override the static model in args with the tier-specific
# model from tool.json.model_tiers, and append the effort flag when configured.
_macc_tier="${MACC_MODEL_TIER:-}"
_macc_routing_mode="${MACC_MODEL_ROUTING_MODE:-auto}"
_tier_model=""
_tier_effort=""
_effort_flag=""

if [[ "$_macc_routing_mode" == "auto" && -n "$_macc_tier" ]]; then
  _tier_model="$(jq -r --arg t "$_macc_tier" '.model_tiers[$t].model // empty' "$tool_json" 2>/dev/null || true)"
  _tier_effort="$(jq -r --arg t "$_macc_tier" '.model_tiers[$t].effort // empty' "$tool_json" 2>/dev/null || true)"
  _effort_flag="$(jq -r '.performer.effort_flag // empty' "$tool_json" 2>/dev/null || true)"
fi

# Strategy A: replace the value after --model / -m in an args array.
# Sets _tier_model_applied_via_args=true when the flag was found and replaced.
_tier_model_applied_via_args=false
_replace_model_in_args() {
  # $1 = name of array variable to modify (passed by name via nameref)
  local -n _arr_ref="$1"
  local _new=() _found=false
  local _a
  local _skip=false
  for _a in "${_arr_ref[@]}"; do
    if $_skip; then
      _new+=("$_tier_model"); _skip=false; _found=true
    elif [[ "$_a" == "--model" || "$_a" == "-m" ]]; then
      _new+=("$_a"); _skip=true
    else
      _new+=("$_a")
    fi
  done
  _arr_ref=("${_new[@]}")
  $_found && _tier_model_applied_via_args=true
}

# Strategy B: write the tier model directly to the tool's config file.
# Used when the tool reads model from a settings file instead of a CLI flag.
_apply_tier_model_via_config_file() {
  local _cfg_path="$1" _cfg_fmt="$2" _cfg_key="$3" _model_val="$4"
  local _dir="${_cfg_path%/*}"
  [[ "$_dir" != "$_cfg_path" ]] && mkdir -p "$_dir" 2>/dev/null
  case "$_cfg_fmt" in
    toml)
      if [[ -f "$_cfg_path" ]]; then
        # Update existing key if present, otherwise append.
        if grep -qE "^[[:space:]]*${_cfg_key}[[:space:]]*=" "$_cfg_path" 2>/dev/null; then
          local _tmp; _tmp="$(mktemp)"
          sed "s|^[[:space:]]*${_cfg_key}[[:space:]]*=.*|${_cfg_key} = \"${_model_val}\"|" \
              "$_cfg_path" > "$_tmp" && mv "$_tmp" "$_cfg_path"
        else
          printf '\n%s = "%s"\n' "$_cfg_key" "$_model_val" >> "$_cfg_path"
        fi
      else
        printf '%s = "%s"\n' "$_cfg_key" "$_model_val" > "$_cfg_path"
      fi
      ;;
    json)
      local _tmp; _tmp="$(mktemp)"
      if [[ -f "$_cfg_path" ]]; then
        jq --arg k "$_cfg_key" --arg v "$_model_val" '.[$k] = $v' \
           "$_cfg_path" > "$_tmp" && mv "$_tmp" "$_cfg_path"
      else
        jq -n --arg k "$_cfg_key" --arg v "$_model_val" '{($k): $v}' > "$_cfg_path"
      fi
      ;;
  esac
}

# Apply tier model: try arg-replacement first; fall back to config file.
_apply_tier_model_to_args() {
  [[ -z "$_tier_model" ]] && return

  # Strategy A: tool has --model flag in its args array.
  _replace_model_in_args args

  if $_tier_model_applied_via_args; then
    # Append effort/reasoning flag for CLI-flag tools (e.g. codex --reasoning-effort).
    if [[ -n "$_tier_effort" && -n "$_effort_flag" ]]; then
      local _has=false
      for _a in "${args[@]}"; do [[ "$_a" == "$_effort_flag" ]] && _has=true && break; done
      $_has || args+=("$_effort_flag" "$_tier_effort")
    fi
  else
    # Strategy B: tool reads model from a config file (e.g. vibe, agy).
    local _cfg_path _cfg_fmt _cfg_key
    _cfg_path="$(jq -r '.performer.model_config.path // empty' "$tool_json" 2>/dev/null || true)"
    _cfg_fmt="$(jq -r '.performer.model_config.format // empty' "$tool_json" 2>/dev/null || true)"
    _cfg_key="$(jq -r '.performer.model_config.key // "model"' "$tool_json" 2>/dev/null || true)"
    if [[ -n "$_cfg_path" && -n "$_cfg_fmt" ]]; then
      _apply_tier_model_via_config_file "$_cfg_path" "$_cfg_fmt" "$_cfg_key" "$_tier_model"
    fi
  fi
}

_apply_tier_model_to_args

prompt_mode="$(jq -r '.performer.prompt.mode // "stdin"' "$tool_json")"
prompt_arg="$(jq -r '.performer.prompt.arg // empty' "$tool_json")"
prompt_text="$(cat "$prompt_file")"
output_capture="$(mktemp)"
active_session_id=""
sid=""

cleanup_runner() {
  if [[ -n "$active_session_id" ]]; then
    if acquire_session_lock; then
      mark_lease_status "$active_session_id" "available" || true
      release_session_lock
    fi
  fi
  rm -f "$output_capture"
}
trap cleanup_runner EXIT

run_default_call() {
  local final_call_args=()
  local a

  expand_config_args "$sid" final_call_args

  if [[ "$prompt_mode" == "arg" ]]; then
    if [[ -z "$prompt_arg" ]]; then
      echo "Error: performer.prompt.arg required for arg mode" >&2
      return 1
    fi
    run_and_capture "$output_capture" "$command" "${final_call_args[@]}" "$prompt_arg" "$prompt_text"
  else
    local rc=0
    printf '[MACC] invoke (${TOOL_LOG_PREFIX} 2): %s' "$command" >&2
    printf ' %q' "${final_call_args[@]}" >&2
    printf '\n' >&2
    printf "%s" "$prompt_text" | "$command" "${final_call_args[@]}" 2>&1 | tee "$output_capture"
    rc=${PIPESTATUS[1]}
    return "$rc"
  fi
}

# RL-PERFORMER-009: override exit code to 0 when the task reported a result
# via the MACC_TASK_RESULT marker, regardless of transient runner errors.
override_rc_for_success_marker() {
  local rc="$1"
  [[ "$rc" -eq 0 ]] && { echo 0; return; }
  if grep -qE 'MACC_TASK_RESULT:[[:space:]]*(success_with_changes|success_without_changes|already_satisfied|error_with_changes|error_without_changes)' \
      "$output_capture" 2>/dev/null; then
    echo 0
  else
    echo "$rc"
  fi
}

# RL-PERFORMER-010: Detect tool-reported quota/session-limit errors that the
# tool emits as human-readable text but exits 0 (a tool-level bug/design quirk).
#
# When detected on a zero exit code:
#   - Prints a structured MACC_TOOL_LIMIT line to stderr so the coordinator
#     log and the per-adapter error normalizer can classify it as E602
#     (QuotaExhausted / not retryable).
#   - Returns exit code 1 so the runtime sees a failure and invokes the
#     normalizer (normalizer_input is only populated on !success).
#   - Optionally extracts and emits a retry-after hint from the message.
#
# Patterns covered:
#   Codex:  "ERROR: You've hit your usage limit. ... try again at <DATE>"
#   Claude: "You've hit your session limit · resets <TIME>"
detect_tool_limit_exit() {
  local rc="$1"
  [[ "$rc" -ne 0 ]] && { echo "$rc"; return; }
  [[ -f "$output_capture" ]] || { echo "$rc"; return; }

  # Match the usage/session limit phrases both tools emit.
  if ! grep -qiE \
      "(you.ve hit your (usage|session) limit|hit your usage limit|your usage limit)" \
      "$output_capture" 2>/dev/null; then
    echo "$rc"
    return
  fi

  # Try to extract an absolute "try again at <DATE>" hint (codex format).
  # Also handle "resets <TIME>" form (claude format). Emit as a raw string for
  # the normalizer / log — converting to epoch here would require locale-aware
  # date parsing which is fragile across distros.
  local retry_hint=""
  retry_hint="$(grep -oiE \
      "(try again at [A-Za-z]+ [0-9]+[a-z]*, [0-9]+ [0-9]+:[0-9]+ [AaPp][Mm]([^.]*)?|resets [0-9]+:[0-9]+[AaPp][Mm]([^)]*)?)" \
      "$output_capture" 2>/dev/null | head -1 || true)"

  # Emit a structured notification that appears in the performer log and is
  # picked up as the tail text by the normalizer path.
  {
    echo ""
    echo "MACC_TOOL_LIMIT: quota_exhausted tool=${tool_id}${retry_hint:+ retry_hint=\"${retry_hint}\"}"
    echo "The tool '${tool_id}' has exhausted its usage quota and exited 0 without doing any work."
    echo "This is treated as an error (E602) so the coordinator can handle it correctly."
    if [[ -n "$retry_hint" ]]; then
      echo "Retry hint from tool: ${retry_hint}"
    fi
  } >&2

  echo 1
}

if [[ "$session_enabled" == "true" && -n "$session_resume_command" ]]; then
  sid=""
  rc=0

  # When the caller (coordinator performer) already provides a session ID,
  # use it directly instead of reading from tool-sessions.json.  This keeps
  # session authority at the coordinator level while still allowing the tool
  # runner to manage leases.
  if [[ -n "$caller_session_id" ]]; then
    sid="$caller_session_id"
    if acquire_session_lock; then
      write_active_lease "$sid"
      active_session_id="$sid"
      release_session_lock
    fi
  else
    if acquire_session_lock; then
      sid="$(find_available_session_id)"
      if [[ -n "$sid" ]]; then
        write_active_lease "$sid"
        active_session_id="$sid"
      fi
      release_session_lock
    fi
  fi

  if [[ -z "$sid" && "$session_id_strategy" == "generated" ]]; then
    sid=""
    if acquire_session_lock; then
      sid="$(reserve_generated_session_id || true)"
      release_session_lock
    fi
  fi

  if [[ "$attempt" -gt 1 && -z "$sid" ]]; then
    echo "Error: missing session id for retry attempt $attempt" >&2
    exit 1
  fi

  if [[ -n "$sid" ]]; then
    if ! run_resume_and_capture "$output_capture" "$sid" "$prompt_text"; then
      rc=$?
      run_default_call || rc=$?
    fi
  else
    run_default_call || rc=$?
  fi

  new_sid="$(extract_session_id_from_output "$output_capture" "$session_extract_regex")"
  if [[ -n "$sid" && "$attempt" -gt 1 ]]; then
    new_sid="$sid"
  fi
  if [[ -z "$new_sid" && "$attempt" -eq 1 && "$session_id_strategy" == "discovered" ]]; then
    discovery_capture="$(mktemp)"
    new_sid="$(discover_session_id "$discovery_capture")"
    rm -f "$discovery_capture"
  fi
  if [[ -z "$new_sid" && -n "$sid" && "$session_id_strategy" == "generated" ]]; then
    new_sid="$sid"
  fi
  if [[ -n "$new_sid" ]]; then
    if acquire_session_lock; then
      if [[ -n "$sid" && "$new_sid" != "$sid" ]]; then
        # The tool created a new session instead of resuming the claimed one
        # (resume failed or the session had expired on the tool's side).
        # Release the old claim so it doesn't leak as permanently "active".
        mark_lease_status "$sid" "available" || true
        write_active_lease "$new_sid" "resume_failed_fallback"
        active_session_id="$new_sid"
      elif ! session_occupied_by_other "$new_sid"; then
        write_active_lease "$new_sid"
        active_session_id="$new_sid"
      fi
      prune_pool_available "$session_pool_cap"
      release_session_lock
    fi
  fi
  # Apply limit detection before the success-marker override so that a tool
  # that emits a quota error but exits 0 is not silently treated as done.
  rc="$(detect_tool_limit_exit "$rc")"
  exit "$(override_rc_for_success_marker "$rc")"
else
  rc=0
  run_default_call || rc=$?
  rc="$(detect_tool_limit_exit "$rc")"
  exit "$(override_rc_for_success_marker "$rc")"
fi
