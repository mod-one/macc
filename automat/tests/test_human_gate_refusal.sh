#!/usr/bin/env bash
# A human approval gate must never be executed by a performer. The coordinator
# does not dispatch it; this pins the performer-side refusal that protects
# against any other dispatch path.
set -uo pipefail
PERFORMER="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/performer.sh"
failures=0
pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n     %s\n' "$1" "${2:-}"; failures=$((failures + 1)); }
fn="$(sed -n '/^is_human_approval_gate()/,/^}/p' "$PERFORMER")"
check() { bash -c "$fn
is_human_approval_gate '$1'"; }

check '{"id":"SEC-APP-003","gate":{"kind":"human_approval","subject_task":"SEC-ADR-003"}}' \
  && pass "human_approval gate is recognised" || fail "human_approval gate is recognised"
check '{"id":"ACC-1","gate":{"required_verdict":"accepted"}}' \
  && fail "verdict gate must still run" || pass "verdict gate (no kind) is not refused"
check '{"id":"T-1"}' && fail "plain task must run" || pass "plain task is not refused"
grep -q 'is_human_approval_gate "$next_task_json"' "$PERFORMER" \
  && pass "refusal is wired before the prompt is built" || fail "refusal is wired before the prompt is built"
grep -q 'Never produce, simulate, or claim a human approval' "$PERFORMER" \
  && pass "prompt forbids agent-made approvals" || fail "prompt forbids agent-made approvals"
printf '\n%d failure(s)\n' "$failures"; exit $(( failures > 0 ))
