#!/usr/bin/env bash
# Tool-neutral JSON session pool and leases (P0 compatibility store).
# Session-state locking.
#
# This lock guards `.macc/state/tool-sessions.json`, which is also written from
# Rust (`core/src/coordinator/session_manager.rs`). Both sides must therefore
# use the SAME primitive on the SAME path or they do not interlock at all.
#
# Rust uses `flock(2)` on the lock file, so this side does too, via flock(1).
# An earlier `mkdir`-based lock here was doubly wrong: it could not interlock
# with the Rust side (which holds a regular file at that path, making every
# `mkdir` fail forever), and a performer killed mid-run left the directory
# behind, wedging session state until it was removed by hand.
#
# `flock` is released by the kernel when the holding process dies, so neither
# failure mode can recur.
session_lock_fd=9
session_lock_supported=""

session_lock_available() {
  if [[ -z "$session_lock_supported" ]]; then
    if command -v flock >/dev/null 2>&1; then
      session_lock_supported="yes"
    else
      session_lock_supported="no"
      echo "Warning: flock(1) not found; session reuse disabled; starting fresh without pooled writes." >&2
    fi
  fi
  [[ "$session_lock_supported" == "yes" ]]
}

acquire_session_lock() {
  session_lock_available || return 1

  # An older build locked by creating a *directory* here. A leftover one can
  # only be a remnant of a process that died (the old scheme removed it on the
  # way out), and it would make every open below fail, so reclaim it.
  if [[ -d "$session_lock_dir" ]]; then
    rmdir "$session_lock_dir" 2>/dev/null || rm -rf "$session_lock_dir" 2>/dev/null || true
  fi

  # Create the lock file if absent; never remove it. Unlinking a lock file lets
  # a second process create a fresh inode at the same path and lock that
  # instead, so both would believe they hold the lock.
  : >>"$session_lock_dir" 2>/dev/null || {
    echo "Warning: cannot create session lock file: $session_lock_dir" >&2
    return 1
  }

  eval "exec ${session_lock_fd}>>\"\$session_lock_dir\"" 2>/dev/null || return 1
  if ! flock -w 30 -x "$session_lock_fd"; then
    echo "Error: timed out acquiring session lock: $session_lock_dir" >&2
    eval "exec ${session_lock_fd}>&-" 2>/dev/null || true
    return 1
  fi
  if [[ -f "$session_state_file" ]] && ! jq -e --arg tool "$tool_id" '
      type == "object" and ((.tools // {}) | type == "object") and
      ((.tools[$tool] // {}) | type == "object") and
      ((.tools[$tool].sessions // {}) | type == "object")
    ' "$session_state_file" >/dev/null 2>&1; then
    echo "Error: invalid session store; existing state preserved: $session_state_file" >&2
    release_session_lock
    return 1
  fi
}

release_session_lock() {
  session_lock_available || return 0
  # Closing the descriptor releases the flock; the file itself stays in place.
  eval "exec ${session_lock_fd}>&-" 2>/dev/null || true
}

now_iso() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

now_epoch() {
  date -u +%s
}

log_session_event() {
  local event_type="$1"
  local sid="$2"
  local owner_pid="$3"
  local detail="$4"
  local events_file="${repo}/.macc/log/events.jsonl"
  local now seq payload
  now="$(now_iso)"
  seq="$(date +%s%N 2>/dev/null || date +%s)"
  [[ -d "$(dirname "$events_file")" ]] || mkdir -p "$(dirname "$events_file")" 2>/dev/null || true
  payload="$(jq -nc \
    --arg schema "1" \
    --arg event_id "evt-${event_type}-${task_id:-session}-${seq}" \
    --arg run_id "${COORDINATOR_RUN_ID:-}" \
    --argjson seq "$seq" \
    --arg ts "$now" \
    --arg source "performer:runner:${tool_id}" \
    --arg task "${task_id:--}" \
    --arg type "$event_type" \
    --arg phase "session" \
    --arg status "ok" \
    --arg severity "info" \
    --arg sid "$sid" \
    --arg tool "$tool_id" \
    --arg pid "$owner_pid" \
    --arg detail "$detail" \
    --arg role "$session_role" \
    --arg scope "$session_scope" \
    '{
      schema_version: $schema,
      event_id: $event_id,
      run_id: (if $run_id != "" then $run_id else null end),
      seq: ($seq|tonumber?),
      ts: $ts,
      source: $source,
      task_id: $task,
      type: $type,
      phase: $phase,
      status: $status,
      severity: $severity,
      payload: {
        session_id: $sid,
        tool: $tool,
        role: $role,
        scope: $scope,
        task_id: (if $task != "-" then $task else null end),
        owner_pid: (if $pid != "" then ($pid|tonumber?) else null end),
        message: ("session " + $sid + " tool=" + $tool + " task=" + $task + " role=" + $role + " scope=" + $scope + " pid=" + $pid + ": " + $detail),
        detail: $detail
      }
    }')"
  echo "$payload" >> "$events_file" 2>/dev/null || true
}

# A live or ambiguous owner stays occupied regardless of heartbeat age.
session_occupied_by_other() {
  local sid="$1"
  local status
  [[ -n "$sid" ]] || return 1
  [[ -f "$session_state_file" ]] || return 1

  status="$(jq -r --arg tool "$tool_id" --arg sid "$sid" \
    '(.tools[$tool].sessions[$sid].status // "available")' \
    "$session_state_file" 2>/dev/null)"
  [[ "$status" == "active" ]] || return 1

  # A dead runner does not prove its provider child has exited. Unclean
  # leases stay ineligible until the coordinator quarantines them; P0-B will
  # add fenced recovery with provider/process verification.

  return 0
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
    if session_can_resume "$sid"; then
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
  if [[ -f "$session_state_file" ]] && jq -e --arg tool "$tool_id" --arg sid "$sid" \
      '.tools[$tool].sessions[$sid] != null' "$session_state_file" >/dev/null; then
    session_identity_matches "$sid" || return 1
    local existing_status
    existing_status="$(jq -r --arg tool "$tool_id" --arg sid "$sid" '.tools[$tool].sessions[$sid].status' "$session_state_file")"
    [[ "$existing_status" == "available" || "$existing_status" == "active" ]] || return 1
    if session_occupied_by_other "$sid"; then
      local owner
      owner="$(jq -r --arg tool "$tool_id" --arg sid "$sid" '.tools[$tool].sessions[$sid].owner_pid' "$session_state_file")"
      [[ "$owner" == "$$" ]] || return 1
    fi
  fi
  local now ts tmp prev_uses
  now="$(now_iso)"
  ts="$(now_epoch)"
  tmp="$(mktemp "${session_state_file}.tmp.XXXXXX")"

  prev_uses="$(jq -r --arg tool "$tool_id" --arg sid "$sid" \
    '(.tools[$tool].sessions[$sid].use_count // 0)' "$session_state_file" 2>/dev/null || echo "0")"
  [[ "$prev_uses" =~ ^[0-9]+$ ]] || prev_uses=0

  if [[ -f "$session_state_file" ]]; then
    jq \
      --arg tool "$tool_id" \
      --arg sid "$sid" \
      --arg now "$now" \
      --arg wt "$worktree" \
      --arg tid "$task_id" \
      --arg pid "$$" \
      --arg reason "$creation_reason" \
      --argjson identity "$session_identity" \
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
          session_identity: $identity,
          status: "active",
          owner_worktree: $wt,
          owner_task_id: $tid,
          owner_pid: $pid,
          heartbeat_epoch: $hb,
          updated_at: $now
        }
      )
      ' "$session_state_file" >"$tmp" || { rm -f "$tmp"; return 1; }
  else
    jq -n \
      --arg tool "$tool_id" \
      --arg sid "$sid" \
      --arg now "$now" \
      --arg wt "$worktree" \
      --arg tid "$task_id" \
      --arg pid "$$" \
      --arg reason "$creation_reason" \
      --argjson identity "$session_identity" \
      --argjson hb "$ts" '{
        tools: {
          ($tool): {
            sessions: {
              ($sid): {
                session_identity: $identity,
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
      }' >"$tmp" || { rm -f "$tmp"; return 1; }
  fi

  mv "$tmp" "$session_state_file" || { rm -f "$tmp"; return 1; }

  if [[ "$prev_uses" -eq 0 || "$creation_reason" == "generated" || "${sid_is_new:-0}" -eq 1 ]]; then
    log_session_event "session_reserved" "$sid" "$$" "new session reserved"
  else
    log_session_event "session_reused" "$sid" "$$" "reused session (use_count=$((prev_uses + 1)))"
  fi
}

# Remove the oldest available (non-active) sessions for this tool so the pool
# stays under $1 entries. Active sessions are never touched. Old-format entries
# (with a nested session_id field) are treated as non-prunable and kept.
prune_pool_available() {
  local cap="$1"
  [[ -f "$session_state_file" ]] || return 0
  local tmp
  tmp="$(mktemp "${session_state_file}.tmp.XXXXXX")"
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
  tmp="$(mktemp "${session_state_file}.tmp.XXXXXX")"
  jq \
    --arg tool "$tool_id" \
    --arg sid "$sid" \
    --arg status "$status" \
    --arg now "$now" \
    --arg tid "${task_id:-}" \
    --arg pid "$$" \
    --argjson identity "$session_identity" \
    --argjson hb "$ts" '
    .tools = (.tools // {}) |
    .tools[$tool] = (.tools[$tool] // {}) |
    .tools[$tool].sessions = (.tools[$tool].sessions // {}) |
    if (.tools[$tool].sessions[$sid].owner_pid == $pid and
        .tools[$tool].sessions[$sid].session_identity == $identity) then
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
    ' "$session_state_file" >"$tmp" || { rm -f "$tmp"; return 1; }
  mv "$tmp" "$session_state_file" || { rm -f "$tmp"; return 1; }
  if [[ "$status" == "available" ]]; then
    log_session_event "session_released" "$sid" "$$" "released session lease back to available"
  fi
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
  return 1
}

reserve_generated_session_id() {
  local attempts=0
  local sid=""
  while [[ "$attempts" -lt 10 ]]; do
    sid="$(generate_session_id || true)"
    [[ -n "$sid" ]] || {
      attempts=$((attempts + 1))
      continue
    }
    # A caller-chosen ID must be NEW even if an existing record is available.
    # Do not recreate a provider transcript or overwrite another identity.
    if [[ -f "$session_state_file" ]] && jq -e --arg tool "$tool_id" --arg sid "$sid" \
        '.tools[$tool].sessions[$sid] != null' "$session_state_file" >/dev/null; then
      attempts=$((attempts + 1))
      continue
    fi
    if write_active_lease "$sid" "generated"; then
      active_session_id="$sid"
      printf "%s" "$sid"
      return 0
    fi
    attempts=$((attempts + 1))
  done
  return 1
}

