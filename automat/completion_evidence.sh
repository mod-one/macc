#!/usr/bin/env bash
# Sourced by performer.sh: durable proof before a no-change success is published.
COMPLETION_COMMIT_SHA=""
COMPLETION_EVIDENCE_CREATED="false"

record_completion_evidence() {
  local id="$1" title="$2" result="$3" evidence="$4" task_json="$5"
  COMPLETION_COMMIT_SHA=""
  COMPLETION_EVIDENCE_CREATED="false"
  if [[ -z "$evidence" || "$evidence" == '<'* ]]; then
    set_last_error "E904" "validation" "${result} requires acceptance criteria and verification evidence in MACC_TASK_RESULT_EXP"
    return 1
  fi
  if [[ -z "${base_ref:-}" ]] || ! git rev-parse --verify "${base_ref}^{commit}" >/dev/null 2>&1; then
    set_last_error "E201" "git" "No valid reference branch supplied for completion evidence"
    return 1
  fi
  local dirty
  dirty="$(git status --porcelain -- . ':!performer.sh' ':!worktree.prd.json')" || return 1
  if [[ -n "$dirty" ]]; then
    set_last_error "E904" "validation" "Tool reported ${result} but its worktree contains source changes"
    return 1
  fi
  local verdict="${7:-}" prior prior_verdict=""
  if jq -e '.gate != null' <<< "$task_json" >/dev/null && [[ "${6:-false}" != true ]]; then
    case "$verdict" in
      accepted|rejected|pending) ;;
      *) set_last_error "E904" "validation" "Completion of a gate requires MACC_TASK_GATE_VERDICT"; return 1 ;;
    esac
  fi
  prior="$(git log "$base_ref" --fixed-strings --grep="[macc:task ${id}]" -1 --format=%H)" || return 1
  if [[ -n "$prior" && -n "$verdict" ]]; then
    prior_verdict="$(git log -1 --format=%B "$prior" | sed -n 's/^\[macc:gate_verdict \(.*\)\]$/\1/p')"
  fi
  # Unpublished commits must use the merge lane, even when an older delivery
  # exists. A new gate decision likewise needs its own durable proof.
  if [[ -n "$prior" && ( -z "$verdict" || "$verdict" == "$prior_verdict" ) ]] \
      && git merge-base --is-ancestor HEAD "$base_ref"; then
    COMPLETION_COMMIT_SHA="$prior"
    echo "Verified existing delivery commit: ${prior} (${id})"
    return 0
  fi
  if [[ "${6:-false}" == "true" ]]; then
    set_last_error "E904" "validation" "Passed PRD has no published MACC completion commit; validation is required"
    return 1
  fi
  # A retry after a rejected terminal IPC event reuses its unpublished proof.
  local expected_spec head body
  expected_spec="$(printf '%s' "$task_json" | jq -Sc 'del(.passes)' | sha256sum | awk '{print $1}')" || return 1
  head="$(git rev-parse HEAD)" || return 1
  body="$(git log -1 --format=%B)" || return 1
  if [[ "$body" == *"[macc:task ${id}]"* && "$body" == *"[macc:validation true]"* && "$body" == *"[macc:spec ${expected_spec}]"* && ( -z "$verdict" || "$body" == *"[macc:gate_verdict ${verdict}]"* ) ]] \
      && ! git merge-base --is-ancestor "$head" "$base_ref"; then
    COMPLETION_COMMIT_SHA="$head"
    COMPLETION_EVIDENCE_CREATED="true"
    return 0
  fi
  local message output
  message="$(mktemp)" || return 1
  {
    printf 'chore: %s - Validate completed task\n\n' "$id"
    printf 'Verified task: %s\nResult: %s\nReference: %s\n\nVerification evidence:\n%s\n\n' "$title" "$result" "$base_ref" "$evidence"
    printf '[macc:task %s]\n[macc:phase dev]\n[macc:tool %s]\n[macc:result %s]\n[macc:validation true]\n[macc:spec %s]\n' "$id" "$tool" "$result" "$expected_spec"
    if [[ -n "$verdict" ]]; then printf '[macc:gate_verdict %s]\n' "$verdict"; fi
  } > "$message"
  if ! output="$(git commit --allow-empty --only --file "$message" 2>&1)"; then
    rm -f "$message"
    set_last_error "E201" "git" "Validation commit failed: ${output:0:240}"
    return 1
  fi
  rm -f "$message"
  COMPLETION_COMMIT_SHA="$(git rev-parse HEAD)" || return 1
  COMPLETION_EVIDENCE_CREATED="true"
  soft_emit_performer_event "commit_created" "dev" "done" "$(jq -nc --arg sha "$COMPLETION_COMMIT_SHA" --arg result "$result" '{sha:$sha,result_kind:$result,validation:true}')"
  echo "Created validation commit: ${COMPLETION_COMMIT_SHA} (${id})"
}
