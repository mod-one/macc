#!/usr/bin/env bash
# CLI transport only. Eligibility, identity, leases and fencing live in Rust.
session_service() {
  "${MACC_SESSION_SERVICE_BIN:-macc}" --cwd "$repo" session-store "$@"
}
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/checkpoint.sh"
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/usage.sh"
sqlite_heartbeat_pid=""
sqlite_lease_key=""
sqlite_clean=false
provider_pid=""
provider_group=false
provider_capture_dir=""
provider_tee_pid=""
stop_sqlite_heartbeat() {
  if [[ -n "$sqlite_heartbeat_pid" ]]; then
    kill "$sqlite_heartbeat_pid" 2>/dev/null || true
    wait "$sqlite_heartbeat_pid" 2>/dev/null || true
    sqlite_heartbeat_pid=""
  fi
}
cleanup_sqlite_session() {
  stop_sqlite_heartbeat
  if [[ -n "$provider_pid" ]] && kill -0 "$provider_pid" 2>/dev/null; then
    if [[ "$provider_group" == true ]]; then
      kill -TERM -- "-$provider_pid" 2>/dev/null || true
      (sleep 2; kill -KILL -- "-$provider_pid" 2>/dev/null || true) >/dev/null 2>&1 &
    else
      kill -TERM "$provider_pid" 2>/dev/null || true
      (sleep 2; kill -KILL "$provider_pid" 2>/dev/null || true) >/dev/null 2>&1 &
    fi
    local watchdog=$!
    wait "$provider_pid" 2>/dev/null || true
    kill "$watchdog" 2>/dev/null || true
    if [[ "$provider_group" == true ]]; then kill -KILL -- "-$provider_pid" 2>/dev/null || true; fi
    sqlite_clean=false
  fi
  if [[ -n "$provider_tee_pid" ]]; then kill "$provider_tee_pid" 2>/dev/null || true; wait "$provider_tee_pid" 2>/dev/null || true; fi
  if [[ -n "$provider_capture_dir" ]]; then rm -f "$provider_capture_dir/output"; rmdir "$provider_capture_dir" 2>/dev/null || true; fi
  if [[ -n "$sqlite_lease_key" ]]; then
    finish_usage interrupted
    finish_checkpoint interrupted false || true
    if ! jq -nc --argjson key "$sqlite_lease_key" --argjson clean "$sqlite_clean" '$key + {clean:$clean}' |
        session_service release >/dev/null; then
      echo 'Error: session lease could not be released; verified recovery is required' >&2
    fi
    sqlite_lease_key=""
  fi
  unset MACC_O2_PROJECT_ROOT MACC_O2_MCP_SESSION MACC_O2_MCP_GENERATION MACC_O2_MCP_TOKEN
  if [[ -n "$usage_raw_capture" ]]; then rm -f -- "$usage_raw_capture"; fi
  if [[ -n "$checkpoint_prompt_file" ]]; then rm -f -- "$checkpoint_prompt_file"; fi
}
prepare_mcp_access() {
  unset MACC_O2_PROJECT_ROOT MACC_O2_MCP_SESSION MACC_O2_MCP_GENERATION MACC_O2_MCP_TOKEN
  local options key value
  if options="$(printf '%s' "$sqlite_lease_key" | session_service mcp-options)"; then
    while IFS=$'\t' read -r key value; do
      case "$key" in
        MACC_O2_PROJECT_ROOT|MACC_O2_MCP_SESSION|MACC_O2_MCP_GENERATION|MACC_O2_MCP_TOKEN) export "$key=$value" ;;
      esac
    done < <(jq -r '.env | to_entries[]? | [.key,.value] | @tsv' <<<"$options")
  else
    echo 'Warning: scoped memory MCP unavailable; provider execution and Git work retained' >&2
  fi
}
runner_failure() {
  printf 'Error: %s\nMACC_RUNNER_ERROR: %s\nMACC_TASK_RESULT_EXP: %s; existing Git work is retained.\n' "$2" "$1" "$2" >&2
}
run_sqlite_session() {
  local generate=false request grant interval new_sid rc=0
  local checkpoint_rc=0
  prepare_checkpoint || checkpoint_rc=$?
  if [[ "$checkpoint_rc" == 2 ]]; then return 0; elif [[ "$checkpoint_rc" != 0 ]]; then return 1; fi
  if [[ "$session_enabled" == true && -n "$session_resume_command" && "$session_id_strategy" == generated ]] && jq -e '.performer.session.create.command and .performer.session.create.args' "$tool_json" >/dev/null; then
    generate=true
  fi
  request="$(jq -nc --argjson identity "$session_identity" --arg request "runner-$$-$(date +%s)-${RANDOM}" \
    --arg checkpoint "$checkpoint_id" --argjson pid "$$" --arg preferred "$caller_session_id" --argjson generate "$generate" '
    {identity:$identity,request_id:$request,owner_pid:$pid,preferred_provider_id:(if $preferred=="" then null else $preferred end),generate_provider_id:$generate,checkpoint_id:(if $checkpoint=="" then null else $checkpoint end)}')"
  grant="$(printf '%s' "$request" | session_service acquire)" || return 1
  sqlite_lease_key="$(jq -c '{session_id,lease_token,generation}' <<<"$grant")"
  sid="$(jq -r '.provider_session_id // empty' <<<"$grant")"
  sid_is_new="$(jq -r 'if .is_new then 1 else 0 end' <<<"$grant")"
  prepare_mcp_access
  local native_options native_key native_value
  if native_options="$(printf '%s' "$sqlite_lease_key" | session_service compaction-options)"; then
    while IFS=$'\t' read -r native_key native_value; do
      if [[ "$native_key" =~ ^[A-Z_]+$ ]]; then export "$native_key=$native_value"; fi
    done < <(jq -r '.env | to_entries[]? | [.key,.value] | @tsv' <<<"$native_options")
  else
    echo 'Warning: native auto settings unavailable; existing configuration and checkpoint rotation retained' >&2
  fi
  interval="$(jq -r '.heartbeat_seconds' <<<"$grant")"
  log_session_event "$(jq -r 'if .is_new then "session_reserved" else "session_reused" end' <<<"$grant")" "${sid:-pending}" "$$" "SQLite logical session $(jq -r .session_id <<<"$grant") generation=$(jq -r .generation <<<"$grant")"
  local owner="$$"
  (
    local sleeper=""
    trap '[[ -z "$sleeper" ]] || kill "$sleeper" 2>/dev/null; exit 0' TERM INT
    while true; do
      sleep "$interval" & sleeper=$!
      wait "$sleeper" || exit 0
      sleeper=""
      kill -0 "$owner" 2>/dev/null || exit 0
      if ! printf '%s' "$sqlite_lease_key" | session_service heartbeat >/dev/null; then
        echo 'Error: session lease heartbeat failed; stopping runner' >&2
        kill -TERM "$owner" 2>/dev/null || true
        exit 1
      fi
    done
  ) &
  sqlite_heartbeat_pid=$!
  if [[ -n "$sid" && "$session_enabled" == true && -n "$session_resume_command" ]]; then
    run_resume_and_capture "$output_capture" "$sid" "$prompt_text" || rc=$?
  else
    run_default_call || rc=$?
  fi
  project_usage_output || { runner_failure E103 "Structured output projection failed"; return 1; }
  # No hidden resume fallback: an invocation may already have changed the worktree.
  new_sid="$(extract_session_id_from_output "$output_capture" "$session_extract_regex")"
  if [[ -n "$new_sid" ]]; then
    if ! jq -nc --argjson key "$sqlite_lease_key" --arg sid "$new_sid" '$key+{provider_session_id:$sid}' |
        session_service bind >/dev/null; then
      runner_failure E901 "Ambiguous provider identity or stale lease"
      return 1
    fi
  fi
  local invocation_rc="$rc"
  rc="$(override_rc_for_success_marker "$rc")"
  rc="$(detect_tool_limit_exit "$rc")"
  local acknowledged=true result=success
  if [[ "$checkpoint_enabled" == true && "$sid_is_new" == 1 ]] && ! checkpoint_acknowledged; then
    acknowledged=false
    if [[ "$rc" == 0 ]]; then
      runner_failure E103 "Fresh session did not acknowledge the current checkpoint task and phase"
      rc=1
    fi
  fi
  if [[ "$rc" != 0 ]]; then result=failure; fi
  if grep -qiE "(individual quota reached|hit your (usage|session) limit|MACC_TOOL_LIMIT:)" "$output_capture"; then result=quota; fi
  if grep -Eq '^MACC_TASK_RESULT: *precondition_unmet' "$output_capture"; then result=precondition_unmet; fi
  finish_usage "$result"
  # The shared review protocol is provider-neutral. This records an observation,
  # not a review postcheck, task transition or permission to deliver.
  if [[ "$result" == success && "${MACC_SESSION_ROLE:-}" == reviewer ]]; then
    local review_verdict
    review_verdict="$(sed -n 's/^[[:space:]]*REVIEW_VERDICT:[[:space:]]*//p' "$output_capture" | tail -1 | tr '[:lower:]' '[:upper:]' | sed 's/[[:space:]]*$//')"
    if [[ "$review_verdict" == CHANGES_REQUESTED ]]; then result=changes_requested; fi
  fi
  checkpoint_rc=0
  finish_checkpoint "$result" "$acknowledged" || checkpoint_rc=$?
  if [[ "$checkpoint_rc" == 2 ]]; then return 0; elif [[ "$checkpoint_rc" != 0 ]]; then return 1; fi
  sqlite_clean=true
  if [[ "$session_enabled" != true || -z "$session_resume_command" || "$acknowledged" == false || "$invocation_rc" -ne 0 || ( "$sid_is_new" == 1 && "$rc" -ne 0 ) ]]; then
    sqlite_clean=false
  fi
  # Release explicitly so lease failure cannot be hidden behind a success marker.
  stop_sqlite_heartbeat
  if ! jq -nc --argjson key "$sqlite_lease_key" --argjson clean "$sqlite_clean" '$key+{clean:$clean}' |
      session_service release >/dev/null; then
    runner_failure E901 "Session release failed; transcript left ineligible"
    return 1
  fi
  sqlite_lease_key=""
  return "$rc"
}
