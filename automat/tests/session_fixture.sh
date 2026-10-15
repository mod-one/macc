#!/usr/bin/env bash
# Explicit ownership for runner regression fixtures. Legacy records intentionally
# lack this contract and must be tested as non-resumable, not seeded as trusted.
seed_owned_session() (
  local fixture_repo="$1" fixture_tool_json="$2" fixture_tool="$3"
  local fixture_task="$4" fixture_sid="$5"
  repo="$fixture_repo"; worktree="$fixture_repo"; task_id="$fixture_task"
  tool_json="$fixture_tool_json"; tool_id="$fixture_tool"; session_enabled=true
  session_scope="$(jq -r '.performer.session.scope // "task_role_tool"' "$tool_json")"
  source "$REPO_ROOT/adapters/shared/session_identity.sh"
  initialize_session_identity
  mkdir -p "$repo/.macc/state"
  jq -n --arg tool "$tool_id" --arg sid "$fixture_sid" \
    --arg now "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson identity "$session_identity" '
    {tools:{($tool):{sessions:{($sid):{session_identity:$identity, status:"available",
      created_at:$now, updated_at:$now, last_used_at:$now, use_count:25}}}}}
  ' > "$repo/.macc/state/tool-sessions.json"
)
