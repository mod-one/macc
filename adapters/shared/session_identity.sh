#!/usr/bin/env bash
# Versioned contract mirrored by core::coordinator::session_identity.
# Unknown metadata is never evidence that a transcript belongs to this task.
initialize_session_identity() {
  session_role="${MACC_SESSION_ROLE:-performer}"
  local project_root execution_root
  project_root="$(cd -- "$repo" && pwd -P)"
  execution_root="$(cd -- "$worktree" && pwd -P)"
  session_identity="$(jq -nc --slurpfile spec "$tool_json" \
    --arg project "$project_root" --arg wt "$execution_root" \
    --arg task "$task_id" --arg role "$session_role" --arg tool "$tool_id" \
    --arg scope "$session_scope" --arg tier "${MACC_MODEL_TIER:-}" \
    --arg routing "${MACC_MODEL_ROUTING_MODE:-auto}" '
    {version: 1, project_root: $project, execution_root: $wt, task_id: $task,
     role: $role, tool_id: $tool, scope: $scope,
     execution_config: {performer: $spec[0].performer, model_tiers: ($spec[0].model_tiers // null),
                        tier: $tier, routing: $routing}}')"
  case "$session_scope" in
    task_role_tool|task|worktree|project) ;;
    *) echo "Warning: unsupported session scope '$session_scope'; pooled sessions disabled" >&2
       session_enabled=false ;;
  esac
  if [[ -z "$task_id" || -z "$session_role" ]]; then
    echo "Warning: no task/role identity; pooled sessions disabled" >&2
    session_enabled=false
  fi
}

session_identity_matches() {
  [[ -f "$session_state_file" && -n "$task_id" ]] || return 1
  jq -e --arg tool "$tool_id" --arg sid "$1" --argjson identity "$session_identity" '
    .tools[$tool].sessions[$sid] as $entry |
    $entry.session_id == null and $entry.session_identity == $identity
  ' "$session_state_file" >/dev/null 2>&1
}

session_can_resume() {
  local sid="$1" status
  session_identity_matches "$sid" || return 1
  status="$(jq -r --arg tool "$tool_id" --arg sid "$sid" \
    '.tools[$tool].sessions[$sid].status // "unknown"' "$session_state_file")"
  [[ "$status" == "available" || "$status" == "active" ]] || return 1
  # Explicit caller hints obey the same expiry policy as pool selection.
  jq -e --arg tool "$tool_id" --arg sid "$sid" \
    --argjson now "$(now_epoch)" --argjson max_age "$session_max_age_seconds" '
    (.tools[$tool].sessions[$sid] | .last_used_at // .updated_at // .created_at // "") as $ts |
    (try ($ts | strptime("%Y-%m-%dT%H:%M:%SZ") | mktime) catch -1) as $epoch |
    $epoch >= 0 and ($now - $epoch) <= $max_age
  ' "$session_state_file" >/dev/null 2>&1 || return 1
  ! session_occupied_by_other "$sid"
}
