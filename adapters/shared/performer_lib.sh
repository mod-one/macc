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
session_scope="$(jq -r '.performer.session.scope // "task_role_tool"' "$tool_json")"
session_extract_regex="$(jq -r '.performer.session.extract_regex // "^session[[:space:]]+id:[[:space:]]*([[:alnum:]-]+)[[:space:]]*$"' "$tool_json")"
session_resume_command="$(jq -r '.performer.session.resume.command // empty' "$tool_json")"
# Command that opens a NEW session under a caller-chosen id. Distinct from
# resume for tools where those are different flags (claude: --session-id vs -r).
# Missing create capability means a normal fresh invocation, never guessed flags.
session_create_command="$(jq -r '.performer.session.create.command // empty' "$tool_json")"
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

# Session policy is shared across all adapter runners; provider syntax stays in tool.json.
source "$(dirname -- "${BASH_SOURCE[0]}")/session_identity.sh"
source "$(dirname -- "${BASH_SOURCE[0]}")/session_store.sh"
source "$(dirname -- "${BASH_SOURCE[0]}")/session_sqlite.sh"
session_backend=json
if [[ -f "$session_state_file" ]]; then
  session_backend="$(jq -er --arg root "$(cd -- "$repo" && pwd -P)" '
    if .session_store == null then "json"
    else .session_store | select(.version == 1 and .project_root == $root) | .mode | select(. == "json" or . == "sqlite") end
  ' "$session_state_file")" || { echo 'Error: invalid session store or authority; refusing fallback' >&2; exit 1; }
elif [[ -f "${repo}/.macc/memory.db" ]]; then
  echo 'Error: session authority marker is missing; refusing JSON fallback' >&2
  exit 1
fi
initialize_session_identity
if [[ "$session_backend" == json && "$session_enabled" == "true" ]] && ! session_lock_available; then
  # Fresh invocation is safe; pooled reads/writes without a shared lock are not.
  session_enabled=false
fi

extract_session_id_from_output() {
  local output_file="$1"
  local regex="$2"
  local found=""
  shopt -s nocasematch
  while IFS= read -r line; do
    if [[ "$line" =~ $regex ]]; then
      if [[ -n "$found" && "$found" != "${BASH_REMATCH[1]}" ]]; then
        # Conflicting metadata cannot establish which conversation was created.
        shopt -u nocasematch
        return 0
      fi
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
  [[ "${MACC_DEBUG:-0}" == "1" ]] && printf '[MACC] invoke (%s 1): %s\n' "${TOOL_LOG_PREFIX}" "$*" >&2
  if [[ "$session_backend" == sqlite ]]; then
    local fifo tee_pid
    local capture_dir
    capture_dir="$(mktemp -d "${output_file}.pipe.XXXXXX")"
    provider_capture_dir="$capture_dir"
    fifo="$capture_dir/output"
    mkfifo "$fifo" || { rmdir "$capture_dir"; return 1; }
    tee "$output_file" <"$fifo" &
    tee_pid=$!
    provider_tee_pid="$tee_pid"
    if command -v setsid >/dev/null 2>&1; then
      setsid "$@" >"$fifo" 2>&1 &
      provider_group=true
    else
      "$@" >"$fifo" 2>&1 &
      provider_group=false
    fi
    provider_pid=$!
    wait "$provider_pid" || rc=$?
    if [[ "$provider_group" == true ]]; then kill -KILL -- "-$provider_pid" 2>/dev/null || true; fi
    provider_pid=""
    wait "$tee_pid" || true
    provider_tee_pid=""
    rm -f "$fifo"
    rmdir "$capture_dir"
    provider_capture_dir=""
  else
    "$@" 2>&1 | tee "$output_file"
    rc=${PIPESTATUS[0]}
  fi
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

  # 1. Base args from config.
  #
  # A session id we just reserved does not exist on the tool's side yet, so it
  # must be OPENED, not resumed. For tools where those are different flags
  # (claude: `--session-id <uuid>` creates, `-r <uuid>` continues) using the
  # resume flag on a fresh id fails, and using the create flag on an existing
  # id fails with "Session ID <uuid> is already in use" on every reuse.
  local args_selector=".performer.session.resume.args[]?"
  local invoke_command="$session_resume_command"
  if [[ "$sid_is_new" == "1" ]] \
     && jq -e '.performer.session.create.args' "$tool_json" >/dev/null 2>&1; then
    args_selector=".performer.session.create.args[]?"
    invoke_command="$session_create_command"
  fi
  while IFS= read -r arg; do
    final_args+=("${arg//\{session_id\}/$sid}")
  done < <(jq -r "$args_selector" "$tool_json")

  # 2. Inject attempt-specific flags from retry overrides.
  if [[ "$attempt" -gt 1 && "$sid_is_new" != "1" ]]; then
    while IFS= read -r arg; do
      expanded_retry_args+=("${arg//\{session_id\}/$sid}")
    done < <(jq -r '.performer.retry.args[]?' "$tool_json")
  fi
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

  # Apply the tier effort to the resume/retry invocation as well.
  _replace_effort_in_args final_args

  # Apply tier model override to resume args.
  # Config-file updates (model_config, effort_config) were already done by
  # _apply_tier_model_to_args before the first call — no need to repeat them.
  # Only re-apply the --model arg replacement to the resume-specific arg list.
  if [[ -n "$_tier_model" ]]; then
    local _r_prev_applied="$_tier_model_applied_via_args"
    _tier_model_applied_via_args=false
    _replace_model_in_args final_args
    # Append effort CLI flag if the model was replaced and the tool uses effort_flag
    # (not effort_config — that was already written to disk above).
    if $_tier_model_applied_via_args && [[ -n "$_tier_effort" && -n "$_effort_flag" ]]; then
      # Only use effort_flag when no effort_config is declared for this tool.
      local _ecfg_chk
      _ecfg_chk="$(jq -r '.performer.effort_config.path // empty' "$tool_json" 2>/dev/null || true)"
      if [[ -z "$_ecfg_chk" ]]; then
        local _r_has=false
        for _r_a in "${final_args[@]}"; do [[ "$_r_a" == "$_effort_flag" ]] && _r_has=true && break; done
        $_r_has || final_args+=("$_effort_flag" "$_tier_effort")
      fi
    fi
    _tier_model_applied_via_args="$_r_prev_applied"
  fi

  prepare_usage "$invoke_command" "${final_args[@]}"
  apply_usage_args final_args
  if [[ "$prompt_mode" == "arg" && -n "$prompt_arg" ]]; then
    run_and_capture "$output_file" "$invoke_command" "${final_args[@]}" "$prompt_arg" "$prompt"
  elif [[ "$prompt_mode" == arg ]]; then
    run_and_capture "$output_file" "$invoke_command" "${final_args[@]}" "$prompt"
  else
    run_and_capture "$output_file" "$invoke_command" "${final_args[@]}" <"$prompt_file"
  fi
}

# A retry with no verified session must use the ordinary NEW-session arguments.
# Retry/resume flags are merged only inside run_resume_and_capture.
args=()
while IFS= read -r arg; do
  args+=("$arg")
done < <(jq -r '.performer.args[]?' "$tool_json")

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
    # Model applied via --model CLI arg. Now handle effort:
    #   Priority 1: effort_config (write to config file, e.g. codex's config.toml).
    #   Priority 2: effort_flag (append as CLI arg, for tools that accept it).
    if [[ -n "$_tier_effort" ]]; then
      local _ecfg_path _ecfg_fmt _ecfg_key
      _ecfg_path="$(jq -r '.performer.effort_config.path // empty' "$tool_json" 2>/dev/null || true)"
      _ecfg_fmt="$(jq -r '.performer.effort_config.format // empty' "$tool_json" 2>/dev/null || true)"
      _ecfg_key="$(jq -r '.performer.effort_config.key // "model_reasoning_effort"' "$tool_json" 2>/dev/null || true)"
      local _effort_in_args=false _previous_arg=""
      for _a in "${args[@]}"; do
        if [[ "$_previous_arg" == "-c" || "$_previous_arg" == "--config" ]] \
            && [[ "$_a" == "${_ecfg_key}="* ]]; then
          _effort_in_args=true
        fi
        _previous_arg="$_a"
      done
      if $_effort_in_args; then
        # CLI overrides take precedence. Avoid rewriting TOML (and accidentally
        # modifying a profile/table key or appending the key inside a table).
        _replace_effort_in_args args
      elif [[ -n "$_ecfg_path" && -n "$_ecfg_fmt" ]]; then
        # Write effort to config file — do NOT append a CLI flag.
        _apply_tier_model_via_config_file "$_ecfg_path" "$_ecfg_fmt" "$_ecfg_key" "$_tier_effort"
      elif [[ -n "$_effort_flag" ]]; then
        # Fall back to CLI flag only when no effort_config is declared.
        local _has=false
        for _a in "${args[@]}"; do [[ "$_a" == "$_effort_flag" ]] && _has=true && break; done
        $_has || args+=("$_effort_flag" "$_tier_effort")
      fi
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
    # For config-file-model tools, also write effort to a config file if declared.
    if [[ -n "$_tier_effort" ]]; then
      local _ecfg_path _ecfg_fmt _ecfg_key
      _ecfg_path="$(jq -r '.performer.effort_config.path // empty' "$tool_json" 2>/dev/null || true)"
      _ecfg_fmt="$(jq -r '.performer.effort_config.format // empty' "$tool_json" 2>/dev/null || true)"
      _ecfg_key="$(jq -r '.performer.effort_config.key // "model_reasoning_effort"' "$tool_json" 2>/dev/null || true)"
      if [[ -n "$_ecfg_path" && -n "$_ecfg_fmt" ]]; then
        _apply_tier_model_via_config_file "$_ecfg_path" "$_ecfg_fmt" "$_ecfg_key" "$_tier_effort"
      fi
    fi
  fi
}

# Inline effort override: tools that take reasoning effort as a config
# override argument (codex: `-c model_reasoning_effort="high"`) carry it in
# their args as `<effort_config.key>="<effort>"`, resolved from the tool config
# when tool.json is written. When routing selects a tier effort, rewrite that
# argument without changing the configured default on disk.
# No-op when there is no tier effort, no effort_config key, or no such argument.
_replace_effort_in_args() {
  # $1 = name of array variable to modify (passed by name via nameref)
  [[ -z "$_tier_effort" ]] && return 0
  local _key
  _key="$(jq -r '.performer.effort_config.key // empty' "$tool_json" 2>/dev/null || true)"
  [[ -z "$_key" ]] && return 0
  local -n _eff_ref="$1"
  local _i
  for _i in "${!_eff_ref[@]}"; do
    if [[ "${_eff_ref[$_i]}" == "${_key}="* ]]; then
      _eff_ref[$_i]="${_key}=\"${_tier_effort}\""
    fi
  done
}

_apply_tier_model_to_args
_replace_effort_in_args args

prompt_mode="$(jq -r '.performer.prompt.mode // "stdin"' "$tool_json")"
prompt_arg="$(jq -r '.performer.prompt.arg // empty' "$tool_json")"
prompt_text="$(cat "$prompt_file")"
output_capture="$(mktemp)"
active_session_id=""
# 1 when `sid` was reserved by us this invocation and therefore does not exist
# on the tool's side yet; selects `session.create` over `session.resume`.
sid_is_new=0
sid=""

cleanup_runner() {
  if [[ "$session_backend" == sqlite ]]; then
    cleanup_sqlite_session
  fi
  if [[ -n "$active_session_id" ]]; then
    if acquire_session_lock; then
      mark_lease_status "$active_session_id" "available" || true
      release_session_lock
    fi
  fi
  rm -f "$output_capture"
}
trap cleanup_runner EXIT
trap 'exit 143' TERM
trap 'exit 130' INT

run_default_call() {
  local final_call_args=()
  local a

  expand_config_args "$sid" final_call_args
  prepare_usage "$command" "${final_call_args[@]}"
  apply_usage_args final_call_args

  if [[ "$prompt_mode" == "arg" ]]; then
    if [[ -n "$prompt_arg" ]]; then
      # Explicit flag form, e.g. claude: -p "<prompt>"
      run_and_capture "$output_capture" "$command" "${final_call_args[@]}" "$prompt_arg" "$prompt_text"
    else
      # Bare positional form — prompt appended as the last argument, no flag, no stdin.
      # Used by tools (e.g. Codex) that accept instructions as a plain positional: codex exec "<prompt>"
      run_and_capture "$output_capture" "$command" "${final_call_args[@]}" "$prompt_text"
    fi
  else
    local rc=0
    if [[ "${MACC_DEBUG:-0}" == "1" ]]; then
      printf '[MACC] invoke (%s 2): %s' "${TOOL_LOG_PREFIX}" "$command" >&2
      printf ' %q' "${final_call_args[@]}" >&2
      printf '\n' >&2
    fi
    run_and_capture "$output_capture" "$command" "${final_call_args[@]}" <"$prompt_file" || rc=$?
    return "$rc"
  fi
}

# RL-PERFORMER-009: override exit code to 0 when the task reported a result
# via the MACC_TASK_RESULT marker, regardless of transient runner errors.
override_rc_for_success_marker() {
  local rc="$1"
  [[ "$rc" -eq 0 ]] && { echo 0; return; }
  if grep -qE 'MACC_TASK_RESULT:[[:space:]]*(success_with_changes|success_without_changes|already_satisfied|error_with_changes|error_without_changes|precondition_unmet)' \
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

if [[ "$session_backend" == sqlite && -n "$task_id" ]]; then
  run_sqlite_session && exit 0 || exit $?
fi

if [[ "$session_enabled" == "true" && -n "$session_resume_command" ]]; then
  sid=""
  rc=0

  # Caller IDs are hints, never authority. Check identity and occupancy under
  # the SAME lock used to claim, including for IDs supplied by the coordinator.
  if acquire_session_lock; then
    if [[ -n "$caller_session_id" ]] && session_can_resume "$caller_session_id"; then
      sid="$caller_session_id"
    else
      if [[ -n "$caller_session_id" ]]; then
        log_session_event "session_resume_rejected" "$caller_session_id" "$$" "identity mismatch, unknown ownership, closed state or active lease"
      fi
      sid="$(find_available_session_id)"
    fi
    if [[ -n "$sid" ]]; then
      write_active_lease "$sid"
      active_session_id="$sid"
    elif [[ "$session_id_strategy" == "generated" ]] \
         && jq -e '.performer.session.create.command and .performer.session.create.args' "$tool_json" >/dev/null; then
      sid="$(reserve_generated_session_id || true)"
      if [[ -n "$sid" ]]; then
        sid_is_new=1
        active_session_id="$sid"
      fi
    fi
    release_session_lock
  else
    echo "Error: session state could not be locked; refusing an unsafe resume" >&2
    exit 1
  fi

  if [[ -n "$sid" ]]; then
    if run_resume_and_capture "$output_capture" "$sid" "$prompt_text"; then
      rc=0
    else
      rc=$?
      if [[ "${TOOL_RESUME_FALLBACK:-true}" == "true" ]]; then
        acquire_session_lock || exit 1
        mark_lease_status "$sid" "available"
        release_session_lock
        active_session_id=""
        sid=""
        sid_is_new=0
        run_default_call && rc=0 || rc=$?
      fi
    fi
  else
    run_default_call || rc=$?
  fi

  new_sid="$(extract_session_id_from_output "$output_capture" "$session_extract_regex")"
  # Only an ID returned by this invocation or successfully resumed/created is
  # eligible. A global provider session listing cannot establish provenance.
  if [[ -z "$new_sid" && -n "$sid" && "$rc" -eq 0 ]]; then
    new_sid="$sid"
  fi
  if [[ -n "$new_sid" ]]; then
    if acquire_session_lock; then
      if [[ -n "$sid" && "$new_sid" != "$sid" ]]; then
        # The tool created a new session instead of resuming the claimed one
        # (resume failed or the session had expired on the tool's side).
        # Release the old claim so it doesn't leak as permanently "active".
        mark_lease_status "$sid" "available" || true
        if write_active_lease "$new_sid" "resume_failed_fallback"; then
          active_session_id="$new_sid"
        fi
      elif ! session_occupied_by_other "$new_sid"; then
        if write_active_lease "$new_sid"; then
          active_session_id="$new_sid"
        fi
      fi
      prune_pool_available "$session_pool_cap"
      release_session_lock
    fi
  fi
  # Apply success-marker override first, then quota detection.
  #
  # ORDER MATTERS:
  # 1. override_rc_for_success_marker: may flip a non-zero rc back to 0 if
  #    the tool printed a MACC_TASK_RESULT marker (e.g. task completed before
  #    a transient error).
  # 2. detect_tool_limit_exit: ALWAYS runs last so a quota/session-limit error
  #    in the output overrides even a success marker.  This prevents session
  #    contamination: resumed sessions may contain MACC_TASK_RESULT markers
  #    from earlier interactions, which must not suppress a quota failure.
  rc="$(override_rc_for_success_marker "$rc")"
  rc="$(detect_tool_limit_exit "$rc")"
  exit "$rc"
else
  rc=0
  run_default_call || rc=$?
  rc="$(override_rc_for_success_marker "$rc")"
  rc="$(detect_tool_limit_exit "$rc")"
  exit "$rc"
fi
