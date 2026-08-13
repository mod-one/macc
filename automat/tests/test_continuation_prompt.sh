#!/usr/bin/env bash
# Regression tests for continuation-attempt support in performer.sh.
#
# Background: a tool that commits work and then reports `error_with_changes` is
# re-dispatched into the worktree it already holds. Before this existed, the
# resumed run received the identical from-scratch prompt -- a cold model asked
# to "implement the task" in a worktree that already contained half of its own
# implementation. These tests pin the three pieces that prevent that:
#
#   1. a terminal error result always carries an explanation
#   2. the resume signal reaches the prompt builder
#   3. the continuation section names the prior work and how to proceed

set -uo pipefail

PERFORMER="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/performer.sh"
failures=0

pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n     %s\n' "$1" "${2:-}"; failures=$((failures + 1)); }

# Extract the shell functions under test into a standalone harness, so we can
# exercise them without launching a real tool.
extract_fns() {
  sed -n \
    -e '/^extract_task_result_exp()/,/^}/p' \
    -e '/^resolve_task_result_exp()/,/^}/p' \
    -e '/^previous_result_explanation()/,/^}/p' \
    -e '/^build_prior_work_summary()/,/^}/p' \
    -e '/^build_continuation_section()/,/^}/p' \
    "$PERFORMER"
}

make_repo() {
  local dir="$1"
  rm -rf "$dir" && mkdir -p "$dir"
  git -C "$dir" init -q -b main .
  git -C "$dir" config user.email test@example.com
  git -C "$dir" config user.name "Test"
  echo base >"$dir/base.txt"
  git -C "$dir" add . >/dev/null
  git -C "$dir" commit -qm init
}

run_harness() {
  # $1 = worktree, $2 = resume_attempt, $3 = base_ref, $4 = shell snippet
  local wt="$1" attempt="$2" base="$3" snippet="$4"
  ( cd "$wt" && bash -c "
      worktree='$wt'
      resume_attempt='$attempt'
      base_ref='$base'
      performer_log_dir='$wt/.macc/log/performer'
      task_log_path() { echo \"\${performer_log_dir}/\$1.md\"; }
      log_task_line() { :; }
      $(extract_fns)
      $snippet
    " )
}

tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

# ── 1. explanation is never silently empty on a terminal error ───────────────
out_file="$tmp_root/out.txt"
printf 'some tool output\nMACC_TASK_RESULT: error_with_changes\n' >"$out_file"
got="$(run_harness "$tmp_root" 0 "" "resolve_task_result_exp '$out_file' 'error_with_changes'" 2>/dev/null)"
if [[ "$got" == *"no explanation provided"* ]]; then
  pass "missing MACC_TASK_RESULT_EXP yields an explicit placeholder"
else
  fail "missing MACC_TASK_RESULT_EXP yields an explicit placeholder" "got: '$got'"
fi

printf 'MACC_TASK_RESULT_EXP: sandbox denied network\nMACC_TASK_RESULT: error_with_changes\n' >"$out_file"
got="$(run_harness "$tmp_root" 0 "" "resolve_task_result_exp '$out_file' 'error_with_changes'" 2>/dev/null)"
if [[ "$got" == "sandbox denied network" ]]; then
  pass "a provided explanation is passed through verbatim"
else
  fail "a provided explanation is passed through verbatim" "got: '$got'"
fi

printf 'MACC_TASK_RESULT: success_with_changes\n' >"$out_file"
got="$(run_harness "$tmp_root" 0 "" "resolve_task_result_exp '$out_file' 'success_with_changes'" 2>/dev/null)"
if [[ -z "$got" ]]; then
  pass "success results are not given a placeholder explanation"
else
  fail "success results are not given a placeholder explanation" "got: '$got'"
fi

# ── 2 & 3. continuation section content ─────────────────────────────────────
wt="$tmp_root/wt"
make_repo "$wt"
git -C "$wt" checkout -q -b task/resume
echo work >"$wt/feature.txt"
git -C "$wt" add . >/dev/null
git -C "$wt" commit -qm "feat: partial implementation"
mkdir -p "$wt/.macc/log/performer"
printf -- '- Result kind: error_with_changes\n- Explanation: repo-wide build fails in unrelated workspace\n' \
  >"$wt/.macc/log/performer/T1.md"

section="$(run_harness "$wt" 1 main "build_continuation_section T1" 2>/dev/null)"

check_contains() {
  local needle="$1" label="$2"
  if [[ "$section" == *"$needle"* ]]; then pass "$label"; else fail "$label" "missing: $needle"; fi
}

check_contains "CONTINUATION"                                  "section is marked as a continuation"
check_contains "repo-wide build fails in unrelated workspace"  "prior explanation is included"
check_contains "feat: partial implementation"                  "prior commits are listed"
check_contains "feature.txt"                                   "changed files are listed"
check_contains "Do not re-derive the implementation"           "instructs against redoing the work"
check_contains "unsalvageable"                                 "provides an escape hatch for bad prior work"
check_contains "MACC_TASK_RESULT: error_without_changes"       "escape hatch names the marker to emit"

# A task with no commits must not claim there is prior work to build on.
wt2="$tmp_root/wt2"
make_repo "$wt2"
git -C "$wt2" checkout -q -b task/empty
mkdir -p "$wt2/.macc/log/performer"
section="$(run_harness "$wt2" 1 main "build_continuation_section T2" 2>/dev/null)"
if [[ "$section" == *"treat the task as unstarted"* ]]; then
  pass "no commits on the branch is stated explicitly"
else
  fail "no commits on the branch is stated explicitly" "got: $section"
fi

# ── 4. session lock is flock-based and self-healing ─────────────────────────
LIB="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/adapters/shared/performer_lib.sh"
if grep -q 'mkdir "\$session_lock_dir"' "$LIB"; then
  fail "session lock no longer uses mkdir" "performer_lib.sh still mkdir-locks the session file"
else
  pass "session lock no longer uses mkdir"
fi
if grep -q 'flock -w' "$LIB"; then
  pass "session lock uses flock, matching the Rust side"
else
  fail "session lock uses flock, matching the Rust side" "no flock call found"
fi
if grep -q 'rmdir "\$session_lock_dir"' "$LIB" && ! grep -q 'reclaim' "$LIB"; then
  fail "legacy lock directory is reclaimed" "no reclaim path found"
else
  pass "legacy lock directory is reclaimed"
fi

echo
if [[ "$failures" -gt 0 ]]; then
  echo "FAILED: $failures check(s)"
  exit 1
fi
echo "All continuation-prompt checks passed."
