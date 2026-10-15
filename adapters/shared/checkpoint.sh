#!/usr/bin/env bash
# Provider-neutral checkpoint transport. Context validation and rotation are Rust services.
checkpoint_enabled=false
checkpoint_finished=false
checkpoint_prompt_file=""
checkpoint_ack=""
checkpoint_id=""
checkpoint_phase="${MACC_SESSION_PHASE:-dev}"
prepare_checkpoint() {
  local context
  if ! context="$(jq -nc --argjson identity "$session_identity" --arg phase "$checkpoint_phase" '{identity:$identity,phase:$phase}' | session_service checkpoint-prepare 2>&1)"; then
    checkpoint_precondition "$context"
    return 2
  fi
  checkpoint_enabled="$(jq -r .enabled <<<"$context")"
  [[ "$checkpoint_enabled" == true ]] || return 0
  checkpoint_id="$(jq -r .checkpoint_id <<<"$context")"
  checkpoint_ack="$(jq -r .acknowledgement <<<"$context")"
  checkpoint_prompt_file="$(mktemp "${TMPDIR:-/tmp}/macc-checkpoint-prompt.XXXXXX")" || return 1
  cat -- "$prompt_file" > "$checkpoint_prompt_file"
  jq -r .prompt <<<"$context" >> "$checkpoint_prompt_file"
  prompt_file="$checkpoint_prompt_file"
  prompt_text="$(cat -- "$prompt_file")"
}
finish_checkpoint() {
  [[ "$checkpoint_enabled" == true && "$checkpoint_finished" == false && -n "$sqlite_lease_key" ]] || return 0
  local result="$1" acknowledged="$2"
  checkpoint_finished=true
  local response
  if ! response="$(jq -nc --argjson key "$sqlite_lease_key" --arg phase "$checkpoint_phase" --arg result "$result" --argjson ack "$acknowledged" \
      '$key+{phase:$phase,result:$result,acknowledged:$ack}' | session_service checkpoint-finish 2>&1)"; then
    echo "$response" >&2
    if [[ "$response" == *"requires reconciliation"* ]]; then checkpoint_precondition "$response"; return 2; fi
    echo 'Error: task checkpoint could not be persisted; existing Git work is retained' >&2
    return 1
  fi
  checkpoint_finished=true
}

checkpoint_acknowledged() {
  grep -Fxq -- "$checkpoint_ack" "$output_capture" && return 0
  # Also accept text carried in declared JSON/JSONL output. No provider names.
  jq -Rrs '. as $raw | try (fromjson | .. | strings) catch ($raw | split("\n")[] | fromjson? | .. | strings)' "$output_capture" 2>/dev/null | grep -Fx -- "$checkpoint_ack" >/dev/null
}

checkpoint_precondition() {
  printf 'MACC_TASK_PRECONDITION: Validated task checkpoint required: %s\n' "$(printf '%s' "$1" | tr '\n' ' ')"
  echo 'MACC_TASK_RESULT_EXP: Checkpoint validation requires reconciliation before another AI invocation; existing work is preserved.'
  echo 'MACC_TASK_RESULT: precondition_unmet'
}
