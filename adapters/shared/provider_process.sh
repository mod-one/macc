#!/usr/bin/env bash
# Transport only. The native per-invocation supervisor owns descendant cleanup.
provider_pid=""
provider_capture_dir=""
provider_tee_pid=""
provider_quiesced=true
provider_capture_ok=true

final_message_file=""
final_protocol_file=""
provider_trace="${MACC_PROVIDER_TRACE_FILE:-}"
add_final_message_arg() {
  [[ -n "${TOOL_FINAL_MESSAGE_FLAG:-}" ]] || return 0
  local -n argv_ref="$1"
  if [[ -z "$provider_trace" ]]; then
    local task_dir="${task_id//[^[:alnum:]_-]/_}"
    local log_dir="$repo/.macc/log/performer/${task_dir:-standalone}"
    mkdir -p -- "$log_dir" || return 1
    provider_trace="$(mktemp "$log_dir/provider-trace.XXXXXXXX")" || return 1
  fi
  : >>"$provider_trace" || return 1
  printf '[MACC] Provider trace: %q\n' "$provider_trace" >&2
  if [[ -z "$final_message_file" ]]; then
    final_message_file="$(mktemp)" || return 1
    final_protocol_file="$(mktemp)" || return 1
  else
    : >"$final_message_file" || return 1
  fi
  : >"$final_protocol_file" || return 1
  local arg found=false
  for arg in "${argv_ref[@]}"; do
    if [[ "$arg" == "$TOOL_EVENT_STREAM_FLAG" ]]; then found=true; break; fi
  done
  if [[ "$found" == false ]]; then argv_ref+=("$TOOL_EVENT_STREAM_FLAG"); fi
  argv_ref+=("$TOOL_FINAL_MESSAGE_FLAG" "$final_message_file")
}
# Read the adapter-declared session ID from top-level machine events. Command
# output is an opaque field in those events, even when it contains JSON itself.
provider_stream_session_id() {
  local pointer
  pointer="$(jq -r --arg fallback "$TOOL_EVENT_SESSION_POINTER" '.performer.usage.session_id // $fallback' "$tool_json")" || return 1
  [[ -n "$pointer" ]] || return 0
  jq -Rnr --arg path "$pointer" '
    ($path | ltrimstr("/") | split("/") | map(gsub("~1";"/") | gsub("~0";"~"))) as $keys |
    [inputs | fromjson? | objects | getpath($keys)? | strings | select(test("^[a-zA-Z0-9-]+$"))] |
    unique | if length == 1 then .[0] else empty end
  ' "$1"
}

# The event stream owns provider failures, even if the process exits zero
# and writes a last message. Never publish markers from a failed turn.
publish_final_message() {
  local output_file="$1" rc="$2" diagnostic
  if [[ "$provider_capture_ok" != true ]]; then
    runner_failure E103 "Incomplete provider trace; see $provider_trace"
    return 1
  fi
  if ! diagnostic="$(runner_output_view provider-errors "$output_file")"; then
    runner_failure E103 "Cannot inspect provider events; see $provider_trace"
    return 1
  fi
  if [[ -n "$diagnostic" ]]; then
    printf '%s\n' "$diagnostic" >&2
  elif [[ -s "$final_message_file" ]]; then
    runner_output_view final-protocol "$final_message_file" >"$final_protocol_file" || return 1
    if [[ "${MACC_PERFORMER_PROTOCOL_ONLY:-0}" == 1 ]]; then
      cat -- "$final_protocol_file" || return 1
    else
      cat -- "$final_message_file" || return 1
    fi
    printf '\n'
    return "$rc"
  elif [[ "$rc" == 0 ]]; then
    runner_failure E103 "Provider returned no final message; see $provider_trace"
  fi
  if [[ "$rc" == 0 ]]; then return 1; fi
  return "$rc"
}

drain_provider_output() {
  if [[ -n "$provider_capture_dir" ]]; then
    if [[ -f "$provider_capture_dir/completion" ]] && [[ "$(cat -- "$provider_capture_dir/completion")" == quiescent-v1 ]]; then
      provider_quiesced=true
    else
      provider_quiesced=false
      # A crashed boundary must not hang output drain or finalize a checkpoint.
      if [[ -n "$provider_tee_pid" ]]; then kill "$provider_tee_pid" 2>/dev/null || true; fi
    fi
  fi
  if [[ -n "$provider_tee_pid" ]]; then
    wait "$provider_tee_pid" 2>/dev/null || provider_capture_ok=false
    provider_tee_pid=""
  fi
  # Inspect only this invocation's drained stdout, before appending stderr.
  checkpoint_event_ack=false
  if [[ -n "${1:-}" && "${checkpoint_enabled:-false}" == true && "${sid_is_new:-0}" == 1 &&
        "$provider_quiesced" == true && "$provider_capture_ok" == true ]] &&
     declare -F provider_checkpoint_ack >/dev/null && provider_checkpoint_ack "$1" "$checkpoint_ack"; then
    checkpoint_event_ack=true
  fi
  if [[ -n "$provider_capture_dir" ]]; then
    if [[ -f "$provider_capture_dir/stderr" ]]; then
      # Append stderr after draining stdout so a diagnostic cannot split a JSON event.
      if [[ -s "$provider_capture_dir/stderr" ]]; then
        { printf '\n'; cat -- "$provider_capture_dir/stderr"; } | tee -a "$output_capture" >>"$provider_trace" || provider_capture_ok=false
      fi
      rm -f -- "$provider_capture_dir/stderr"
    fi
    rm -f "$provider_capture_dir/output"
    rm -f "$provider_capture_dir/completion"
    rmdir "$provider_capture_dir" 2>/dev/null || true
    provider_capture_dir=""
  fi
}

stop_provider_process() {
  if [[ -n "$provider_pid" ]]; then
    # Never SIGKILL the supervisor: it must reap even tools which detached from
    # the provider's session. Checkpoint/release happen only after this wait.
    kill -TERM "$provider_pid" 2>/dev/null || true
    wait "$provider_pid" 2>/dev/null || true
    provider_pid=""
  fi
  drain_provider_output
}
