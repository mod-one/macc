#!/usr/bin/env bash
# test_session_resume_flags.sh
#
# Regression test for the session create-vs-resume flag bug.
#
# `claude --session-id <uuid>` CREATES a conversation under a caller-chosen id.
# `claude -r <uuid>` CONTINUES one. The shipped spec used --session-id as the
# RESUME command, so it worked exactly once per id and then failed forever with
#   Error: Session ID <uuid> is already in use.
# Because MACC pools and re-issues session ids (tool-sessions.json, use_count
# 25 on the observed run), every dispatch after the first died instantly as
# error_without_changes: 30 failed dispatches on one task in 11 minutes while
# 24 other tasks never started.
#
# This drives the REAL adapters/shared/performer_lib.sh against the REAL
# registry/tools.d/claude.tool.yaml with a stub `claude` that records its argv,
# and asserts the flag actually chosen in each situation.
#
# Prerequisites: jq, python3 (with PyYAML)
#
# Usage:
#   ./automat/tests/test_session_resume_flags.sh

set -euo pipefail

PASS=0
FAIL=0

log()  { printf '[test_session_resume_flags] %s\n' "$*"; }
pass() { log "PASS: $*"; PASS=$((PASS + 1)); }
fail() { log "FAIL: $*"; FAIL=$((FAIL + 1)); }

require_cmd() {
    if ! command -v "$1" &>/dev/null; then
        log "SKIP: required command '$1' not found — skipping all tests."
        exit 0
    fi
}

require_cmd jq
require_cmd python3
python3 -c 'import yaml' 2>/dev/null || { log "SKIP: PyYAML not available."; exit 0; }

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
SPEC="$REPO_ROOT/registry/tools.d/claude.tool.yaml"
RUNNER="$REPO_ROOT/adapters/claude/claude.performer.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# ── Fixture ───────────────────────────────────────────────────────────────────
# tool.json is generated FROM the shipped spec so this test can never pass
# against a spec that has regressed.
mkdir -p "$WORK/repo/.macc/state"
python3 - "$SPEC" "$WORK/repo/.macc/tool.json" <<'PY'
import json, sys, yaml
spec = yaml.safe_load(open(sys.argv[1]))
# The runner reads {model} out of the resolved runtime config; resolve it the
# way worktree.rs does so the stub sees a concrete argv.
def sub(o):
    if isinstance(o, list):
        return [sub(x) for x in o]
    if isinstance(o, dict):
        return {k: sub(v) for k, v in o.items()}
    if isinstance(o, str):
        return o.replace("{model}", "sonnet")
    return o
spec["performer"] = sub(spec["performer"])
json.dump(spec, open(sys.argv[2], "w"))
PY

echo "do the thing" > "$WORK/prompt.txt"

# Stub `claude` that records argv and emits a terminal result so the runner
# treats the call as a completed invocation.
mkdir -p "$WORK/bin"
cat > "$WORK/bin/claude" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$@" >> "${ARGV_LOG:?ARGV_LOG must be set}"
echo "MACC_TASK_RESULT: success"
exit 0
STUB
chmod +x "$WORK/bin/claude"
export PATH="$WORK/bin:$PATH"

EXISTING_SID="0f74d1b9-9179-4727-9b51-b624ade6bcb8"

run_performer() {
    # $1 = argv log path; remaining args appended to the runner invocation.
    local log="$1"; shift
    : > "$log"
    ARGV_LOG="$log" timeout 60 "$RUNNER" \
        --prompt-file "$WORK/prompt.txt" \
        --tool-json "$WORK/repo/.macc/tool.json" \
        --repo "$WORK/repo" \
        --worktree "$WORK/repo" \
        --task-id T-1 \
        "$@" >/dev/null 2>&1 || true
}

# `--` matters: the flags under test ("-r", "--session-id") would
# otherwise be parsed by grep as its own options.
argv_has() { grep -qxF -- "$1" "$2"; }

seed_existing_session() {
    cat > "$WORK/repo/.macc/state/tool-sessions.json" <<JSON
{"tools":{"claude":{"sessions":{"$EXISTING_SID":{"created_at":"2026-08-28T11:05:11Z",
"creation_reason":"generated","heartbeat_epoch":0,"last_task_id":"T-0",
"last_used_at":"$(date -u +%Y-%m-%dT%H:%M:%SZ)","status":"available",
"updated_at":"$(date -u +%Y-%m-%dT%H:%M:%SZ)","use_count":25}}}}}
JSON
}

# ── 1. Reusing an existing session id ─────────────────────────────────────────
# This is the exact shipped failure: the coordinator hands back a pooled id and
# the runner must CONTINUE it, not try to create it again.
seed_existing_session
LOG="$WORK/argv_reuse.log"
run_performer "$LOG" --attempt 1 --max-attempts 1 --session-id "$EXISTING_SID"

if argv_has "-r" "$LOG" && argv_has "$EXISTING_SID" "$LOG"; then
    pass "reusing a pooled session id resumes with -r"
else
    fail "reusing a pooled session id must resume with -r; argv was: $(tr '\n' ' ' < "$LOG")"
fi

if argv_has "--session-id" "$LOG"; then
    fail "reuse passed --session-id (the create flag) — this is the bug that killed the run"
else
    pass "reuse never passes --session-id"
fi

# ── 2. A freshly reserved id must be CREATED, not resumed ─────────────────────
# id_strategy: generated reserves a uuid the tool has never seen; resuming it
# would fail, so the runner must use session.create.
rm -f "$WORK/repo/.macc/state/tool-sessions.json"
LOG="$WORK/argv_fresh.log"
run_performer "$LOG" --attempt 1 --max-attempts 1

if argv_has "--session-id" "$LOG"; then
    pass "a freshly reserved id is opened with --session-id (session.create)"
else
    fail "a freshly reserved id must be created, not resumed; argv was: $(tr '\n' ' ' < "$LOG")"
fi

if argv_has "-r" "$LOG"; then
    fail "a freshly reserved id must not be resumed with -r"
else
    pass "fresh id never passes -r"
fi

# ── 3. Retry attempts resume too ──────────────────────────────────────────────
# performer.retry.args are merged into the resume invocation on attempt > 1, so
# a create flag there reintroduces the same failure one attempt later.
seed_existing_session
LOG="$WORK/argv_retry.log"
run_performer "$LOG" --attempt 2 --max-attempts 3 --session-id "$EXISTING_SID"

if argv_has "--session-id" "$LOG"; then
    fail "retry attempt passed --session-id; argv was: $(tr '\n' ' ' < "$LOG")"
else
    pass "retry attempt never passes --session-id"
fi

if argv_has "-r" "$LOG"; then
    pass "retry attempt resumes with -r"
else
    fail "retry attempt must resume with -r; argv was: $(tr '\n' ' ' < "$LOG")"
fi

# ── Summary ───────────────────────────────────────────────────────────────────
log "-----------------------------------------"
log "Passed: $PASS  Failed: $FAIL"
[[ $FAIL -eq 0 ]] || exit 1
log "All session flag checks passed."
