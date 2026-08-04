#!/usr/bin/env bash
# test_terminal_payload_construction.sh
#
# Regression test for the terminal phase_result payload construction bug (see
# docs/prd/8_3_MACC_Coordinator_Integrity_Recommendations.md §4.1 / §6.2).
#
# A task could produce commits, run to completion, and still be silently
# recorded as if nothing happened: the jq expression building the terminal
# phase_result payload used `field:($value | select(length>0))` inline in an
# object literal. When $value was empty (e.g. result_exp, which is always
# empty for a successful task), jq's `select` yields `empty`, and a field
# whose value is `empty` collapses the ENTIRE object to no output -- so the
# payload silently became `{}`, missing the mandatory `result_kind` field,
# and the coordinator rejected the event.
#
# This test exercises the exact additive-merge jq pattern now used in
# automat/performer.sh and asserts payload.result_kind survives an empty
# result_exp, for both the "done" and "failed" phase_result payloads.
#
# Prerequisites:
#   - jq in PATH
#
# Usage:
#   ./automat/tests/test_terminal_payload_construction.sh

set -euo pipefail

PASS=0
FAIL=0

log()  { printf '[test_terminal_payload_construction] %s\n' "$*"; }
pass() { log "PASS: $*"; PASS=$((PASS + 1)); }
fail() { log "FAIL: $*"; FAIL=$((FAIL + 1)); }

require_cmd() {
    if ! command -v "$1" &>/dev/null; then
        log "SKIP: required command '$1' not found — skipping all tests."
        exit 0
    fi
}

require_cmd jq

# Mirrors the "done" payload construction in run_tool() (automat/performer.sh).
build_done_payload() {
    local attempt="$1" result_kind="$2" changed="$3" result_exp="$4"
    jq -nc \
        --arg attempt "$attempt" \
        --arg result_kind "$result_kind" \
        --argjson changed "$changed" \
        --arg result_exp "$result_exp" \
        '({
            attempt:($attempt|tonumber?),
            changed:$changed,
            message:"Task completed successfully with repository changes."
        }
        + (if $result_kind != "" then {result_kind:$result_kind} else {} end)
        + (if $result_exp  != "" then {result_exp:$result_exp}  else {} end))'
}

# Mirrors the "failed" payload construction in run_tool() (automat/performer.sh).
build_failed_payload() {
    local attempt="$1" status="$2" code="$3" origin="$4" message="$5" result_exp="$6"
    jq -nc \
        --arg attempt "$attempt" \
        --arg status "$status" \
        --arg code "$code" \
        --arg origin "$origin" \
        --arg message "$message" \
        --arg result_exp "$result_exp" \
        '({
            attempt:($attempt|tonumber?),
            exit_status:($status|tonumber?)
        }
        + (if $code       != "" then {error_code:$code}       else {} end)
        + (if $origin     != "" then {origin:$origin}         else {} end)
        + (if $message    != "" then {message:$message}       else {} end)
        + (if $result_exp != "" then {result_exp:$result_exp} else {} end))'
}

# --- Test 1: empty result_exp must not remove result_kind (the exact bug) ---
payload="$(build_done_payload "1" "success_with_changes" "true" "")"
if [[ -z "$payload" ]]; then
    fail "done payload collapsed to empty output with empty result_exp"
elif [[ "$(jq -r '.result_kind // empty' <<<"$payload")" == "success_with_changes" ]]; then
    pass "done payload keeps result_kind when result_exp is empty"
else
    fail "done payload missing result_kind: $payload"
fi

# --- Test 2: non-empty result_exp is still included ---
payload="$(build_done_payload "2" "error_with_changes" "true" "sandbox denied network")"
if [[ "$(jq -r '.result_exp // empty' <<<"$payload")" == "sandbox denied network" ]]; then
    pass "done payload includes result_exp when non-empty"
else
    fail "done payload missing result_exp: $payload"
fi

# --- Test 3: failed payload with all optional fields empty must not collapse ---
payload="$(build_failed_payload "1" "1" "" "" "" "")"
if [[ -z "$payload" ]]; then
    fail "failed payload collapsed to empty output with all-empty optional fields"
elif [[ "$(jq -r '.exit_status // empty' <<<"$payload")" == "1" ]]; then
    pass "failed payload keeps required fields when all optional fields are empty"
else
    fail "failed payload missing exit_status: $payload"
fi

# --- Test 4: failed payload with error_code present is not dropped by an ---
# --- empty result_exp (the exact failed-path variant of the same bug)    ---
payload="$(build_failed_payload "1" "1" "E101" "runner" "runner exited non-zero" "")"
if [[ "$(jq -r '.error_code // empty' <<<"$payload")" == "E101" ]]; then
    pass "failed payload keeps error_code when result_exp is empty"
else
    fail "failed payload missing error_code: $payload"
fi

log "Results: PASS=$PASS FAIL=$FAIL"
if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
exit 0
