#!/usr/bin/env bash
# Regression tests for the MACC_TASK_PRECONDITION contract in performer.sh.
#
# Background (GTransport, SEC-API-003, 2026-09-21): codex stopped correctly with
# `precondition_unmet` and wrote the unsatisfied conditions as a bulleted list,
# but MACC had no structured channel for that list and recorded only
# "tool execution failed". The operator saw a failure with no cause.
#
# These pin the extraction that feeds `unmet_preconditions` into the
# phase_result payload: order, trimming, CR stripping, de-duplication, and the
# empty case.

set -uo pipefail

PERFORMER="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/performer.sh"
failures=0
pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n     %s\n' "$1" "${2:-}"; failures=$((failures + 1)); }

extract_fns() {
  sed -n \
    -e '/^extract_task_preconditions()/,/^}/p' \
    -e '/^task_preconditions_json()/,/^}/p' \
    "$PERFORMER"
}
run_harness() { bash -c "$(extract_fns)
$1"; }

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
out="$tmp/out.txt"

# 1. order kept, whitespace/CR trimmed, exact duplicates dropped
printf 'codex\nPreconditions remain unmet:\nMACC_TASK_PRECONDITION: SEC-CR-001 is still pending approval\r\nMACC_TASK_PRECONDITION:    Recovery-code persistence has no entity   \nMACC_TASK_PRECONDITION: SEC-CR-001 is still pending approval\nMACC_TASK_RESULT_EXP: decisions unresolved\nMACC_TASK_RESULT: precondition_unmet\n' >"$out"
got="$(run_harness "extract_task_preconditions '$out'")"
want=$'SEC-CR-001 is still pending approval\nRecovery-code persistence has no entity'
[[ "$got" == "$want" ]] && pass "conditions extracted in order, trimmed, de-duplicated" \
  || fail "conditions extracted in order, trimmed, de-duplicated" "got: $(printf '%q' "$got")"

# 2. JSON array carries exactly those items
got="$(run_harness "task_preconditions_json '$out'")"
[[ "$got" == '["SEC-CR-001 is still pending approval","Recovery-code persistence has no entity"]' ]] \
  && pass "payload JSON array matches the extracted list" \
  || fail "payload JSON array matches the extracted list" "got: $got"

# 3. no marker lines -> empty array, never null or a blank string
printf 'MACC_TASK_RESULT_EXP: why\nMACC_TASK_RESULT: precondition_unmet\n' >"$out"
got="$(run_harness "task_preconditions_json '$out'")"
[[ "$got" == '[]' ]] && pass "no precondition lines yields []" || fail "no precondition lines yields []" "got: $got"

# 4. a blank marker value is ignored rather than emitted as ""
printf 'MACC_TASK_PRECONDITION:   \nMACC_TASK_PRECONDITION: real one\n' >"$out"
got="$(run_harness "task_preconditions_json '$out'")"
[[ "$got" == '["real one"]' ]] && pass "blank marker values are dropped" || fail "blank marker values are dropped" "got: $got"

# 5. the prompt tells the tool to emit the marker with precondition_unmet
if grep -q 'MACC_TASK_PRECONDITION: <condition>' "$PERFORMER" \
   && grep -q 'unmet_preconditions:\$unmet' "$PERFORMER"; then
  pass "prompt contract and payload wiring are present in performer.sh"
else
  fail "prompt contract and payload wiring are present in performer.sh"
fi

printf '\n%d failure(s)\n' "$failures"
exit $(( failures > 0 ))
