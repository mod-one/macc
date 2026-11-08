#!/usr/bin/env bash
# Transport only: protocol and provider interpretation live in native Rust.
runner_output_inspection() {
  local scope=()
  if [[ -n "${repo:-}" && -n "${task_id:-}" && -n "${MACC_EVENT_SOURCE:-}" ]]; then
    scope=(--project-root "$repo" --task-id "$task_id" --source "$MACC_EVENT_SOURCE")
  fi
  "${MACC_PROTOCOL_CLI:-${MACC_DECISION_CLI:-${MACC_SESSION_SERVICE_BIN:-macc}}}" runner-output --file "$1" --tool "${tool_id:-${tool:-unknown}}" --exit-code "${2:-1}" "${scope[@]}"
}
terminal_result_lines() {
  runner_output_inspection "$1" | jq -er '.terminal_output'
}
