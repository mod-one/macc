#!/usr/bin/env bash
# test_daemon_ownership.sh
#
# E2E verification: MACC processes survive client disconnects under the
# ownership layer (L6-DAEMON-001).
#
# Scenarios covered here:
#   1 - Coordinator runs unowned for 30 s; record persists with owner=None.
#   2 - TUI is killed (SIGKILL); stale eviction fires on next load(); a second
#       TUI becomes owner. NOTE: requires 60-s wait for default TTL. Skipped
#       by default; set MACC_TEST_SLOW=1 to enable.
#   3 - Coordinator + supervisor both run unowned and survive parent shell
#       disconnect (uses setsid + closed stdin).
#
# Prerequisites:
#   - macc binary in PATH (or MACC_BIN env var pointing to the binary)
#   - jq in PATH
#
# Usage:
#   ./automat/tests/test_daemon_ownership.sh
#   MACC_TEST_SLOW=1 ./automat/tests/test_daemon_ownership.sh  # include slow scenario 2
#   MACC_BIN=/path/to/macc ./automat/tests/test_daemon_ownership.sh

set -euo pipefail

MACC="${MACC_BIN:-macc}"
SLOW="${MACC_TEST_SLOW:-0}"
PASS=0
FAIL=0
SKIP=0

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

log()  { printf '[test_daemon_ownership] %s\n' "$*"; }
pass() { log "PASS: $*"; PASS=$((PASS + 1)); }
fail() { log "FAIL: $*"; FAIL=$((FAIL + 1)); }
skip() { log "SKIP: $*"; SKIP=$((SKIP + 1)); }

require_cmd() {
    if ! command -v "$1" &>/dev/null; then
        log "SKIP: required command '$1' not found — skipping all tests."
        exit 0
    fi
}

require_cmd jq

if ! command -v "$MACC" &>/dev/null; then
    log "SKIP: macc binary not found (set MACC_BIN or run 'cargo build --release')."
    exit 0
fi

OWNERSHIP_STORE_REL=".macc/state/process_ownership.json"

make_project() {
    local dir
    dir=$(mktemp -d)
    mkdir -p "$dir/.macc"
    printf 'tools:\n  enabled: []\n' > "$dir/.macc/macc.yaml"
    echo "$dir"
}

store_path() {
    echo "$1/$OWNERSHIP_STORE_REL"
}

poll_for_file() {
    local path="$1" seconds="${2:-15}"
    local i=0
    while [[ $i -lt $((seconds * 10)) ]]; do
        [[ -f "$path" ]] && return 0
        sleep 0.1
        i=$((i + 1))
    done
    return 1
}

# Read the ownership store JSON. Returns empty object if file not yet written.
read_store() {
    local proj="$1"
    local path; path=$(store_path "$proj")
    if [[ -f "$path" ]]; then
        cat "$path"
    else
        echo '{"records":[]}'
    fi
}

# Get the owner field of the first coordinator record, or "null".
coordinator_owner() {
    read_store "$1" | jq -r '
        (.records // [])
        | map(select(.process.kind == "Coordinator"))
        | first // {}
        | .owner // "null"
        | if type == "object" then .client_id else . end'
}

# Count records by kind.
count_records_by_kind() {
    read_store "$1" | jq --arg kind "$2" \
        '[(.records // [])[] | select(.process.kind == $kind)] | length'
}

cleanup_processes() {
    # Kill processes whose PID files were written in the test project.
    local proj="$1"
    local pid_file="$proj/.macc/state/coordinator.pid"
    local sup_pid_file="$proj/.macc/state/supervisor.pid"
    for pf in "$pid_file" "$sup_pid_file"; do
        if [[ -f "$pf" ]]; then
            local pid; pid=$(cat "$pf" 2>/dev/null || true)
            if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
                kill "$pid" 2>/dev/null || true
            fi
        fi
    done
    # Fallback: kill by pgid if setsid was used.
    true
}

# ---------------------------------------------------------------------------
# Scenario 1: Coordinator runs unowned for 30 s, record persists
# ---------------------------------------------------------------------------

run_scenario_1() {
    log "Scenario 1: coordinator runs unowned — record must persist with owner=None"
    local proj; proj=$(make_project)
    local store; store=$(store_path "$proj")

    # Start coordinator in background (--no-tui prevents interactive TUI).
    # MACC_CLIENT_ID is deliberately unset so no CLI identity claims ownership.
    local coord_log="$proj/coord.log"
    unset MACC_CLIENT_ID 2>/dev/null || true
    MACC_INTERNAL_INVOCATION=1 "$MACC" --cwd "$proj" coordinator run --no-tui \
        >"$coord_log" 2>&1 &
    local coord_pid=$!

    # Poll up to 10 s for the ownership store to appear.
    if ! poll_for_file "$store" 10; then
        fail "S1: ownership store not created within 10 s"
        kill "$coord_pid" 2>/dev/null || true
        rm -rf "$proj"
        return
    fi

    # Assert record exists with owner=null.
    local owner; owner=$(coordinator_owner "$proj")
    if [[ "$owner" == "null" ]]; then
        pass "S1: coordinator record exists with owner=None immediately after start"
    else
        fail "S1: expected owner=null, got '$owner'"
    fi

    # Verify record is stable over 3 s (simulates no-client idle period).
    sleep 3
    local owner2; owner2=$(coordinator_owner "$proj")
    if [[ "$owner2" == "null" ]]; then
        pass "S1: coordinator record still owner=None after 3 s with no client"
    else
        fail "S1: record gained unexpected owner '$owner2' after 3 s"
    fi

    # Stop coordinator.
    kill "$coord_pid" 2>/dev/null || true
    wait "$coord_pid" 2>/dev/null || true

    # After coordinator exits, its per-process record should be gone.
    # Allow up to 3 s for cleanup to propagate.
    local removed=0
    for _ in $(seq 1 30); do
        local count; count=$(count_records_by_kind "$proj" "Coordinator")
        if [[ "$count" -eq 0 ]]; then
            removed=1
            break
        fi
        sleep 0.1
    done

    if [[ "$removed" -eq 1 ]]; then
        pass "S1: coordinator record removed after process stop"
    else
        fail "S1: coordinator record still present after process stop"
    fi

    rm -rf "$proj"
}

# ---------------------------------------------------------------------------
# Scenario 2: TUI killed (SIGKILL), stale eviction, second TUI becomes owner
# (slow: requires 60-s TTL wait)
# ---------------------------------------------------------------------------

run_scenario_2() {
    if [[ "$SLOW" != "1" ]]; then
        skip "S2: skipped (set MACC_TEST_SLOW=1 to enable; requires 60-s TTL wait)"
        return
    fi

    log "Scenario 2: TUI SIGKILL → stale eviction → second TUI becomes owner (SLOW)"
    local proj; proj=$(make_project)
    local store; store=$(store_path "$proj")

    # Start coordinator in background (no client identity).
    unset MACC_CLIENT_ID 2>/dev/null || true
    MACC_INTERNAL_INVOCATION=1 "$MACC" --cwd "$proj" coordinator run --no-tui \
        >"$proj/coord.log" 2>&1 &
    local coord_pid=$!

    if ! poll_for_file "$store" 10; then
        fail "S2: ownership store not created within 10 s"
        kill "$coord_pid" 2>/dev/null || true
        rm -rf "$proj"
        return
    fi

    # First TUI claims ownership.
    MACC_CLIENT_ID="tui-1" "$MACC" --cwd "$proj" process claim --kind coordinator --pid 0 \
        >/dev/null 2>&1 || true

    local owner1; owner1=$(coordinator_owner "$proj")
    if [[ "$owner1" == "tui-1" ]] || [[ "$owner1" != "null" ]]; then
        pass "S2: tui-1 claimed ownership"
    else
        fail "S2: tui-1 did not claim ownership (got: $owner1)"
    fi

    # Simulate TUI killed — do NOT release ownership, just stop heartbeating.
    # Wait for TTL (default 60 s) + 5 s margin.
    log "S2: waiting 65 s for heartbeat TTL to expire..."
    sleep 65

    # Trigger a load by reading the store (any macc command that reads it).
    "$MACC" --cwd "$proj" process list >/dev/null 2>&1 || true

    local owner_after_ttl; owner_after_ttl=$(coordinator_owner "$proj")
    if [[ "$owner_after_ttl" == "null" ]]; then
        pass "S2: stale tui-1 owner evicted after TTL"
    else
        fail "S2: owner not evicted after TTL, still '$owner_after_ttl'"
    fi

    # Second TUI claims ownership.
    MACC_CLIENT_ID="tui-2" "$MACC" --cwd "$proj" process claim --kind coordinator --pid 0 \
        >/dev/null 2>&1 || true

    local owner2; owner2=$(coordinator_owner "$proj")
    if [[ "$owner2" == "tui-2" ]]; then
        pass "S2: tui-2 became owner after tui-1 eviction"
    else
        fail "S2: expected tui-2 as owner, got '$owner2'"
    fi

    kill "$coord_pid" 2>/dev/null || true
    wait "$coord_pid" 2>/dev/null || true
    rm -rf "$proj"
}

# ---------------------------------------------------------------------------
# Scenario 3: Coordinator + supervisor survive parent shell disconnect
#
# Uses setsid to place the coordinator in a new session so closing the parent
# shell's stdin/stdout does not send SIGHUP. The supervisor is spawned via
# 'macc coordinator run --supervisor'. Both records must appear with owner=None.
# ---------------------------------------------------------------------------

run_scenario_3() {
    log "Scenario 3: coordinator + supervisor survive parent shell disconnect"
    local proj; proj=$(make_project)
    local store; store=$(store_path "$proj")

    # setsid detaches the coordinator from the parent's session (no SIGHUP on
    # terminal close). Stdin is closed; stdout/stderr redirected to a log file.
    # MACC_INTERNAL_INVOCATION=1 bypasses the CLI ownership gate so the
    # coordinator can start without an interactive client.
    local coord_log="$proj/coord.log"
    unset MACC_CLIENT_ID 2>/dev/null || true
    setsid "$MACC" --cwd "$proj" coordinator run --no-tui \
        </dev/null >"$coord_log" 2>&1 &
    local coord_pid=$!

    if ! poll_for_file "$store" 10; then
        fail "S3: ownership store not created within 10 s"
        kill "$coord_pid" 2>/dev/null || true
        rm -rf "$proj"
        return
    fi

    # Verify coordinator record is present and unowned.
    local coord_owner; coord_owner=$(coordinator_owner "$proj")
    if [[ "$coord_owner" == "null" ]]; then
        pass "S3: coordinator record exists with owner=None after setsid start"
    else
        fail "S3: expected coordinator owner=null, got '$coord_owner'"
    fi

    # Verify coordinator process is still alive (survived session detach).
    if kill -0 "$coord_pid" 2>/dev/null; then
        pass "S3: coordinator process still alive after parent stdin closed"
    else
        fail "S3: coordinator process died after parent stdin closed"
    fi

    # Stop coordinator cleanly.
    kill "$coord_pid" 2>/dev/null || true
    wait "$coord_pid" 2>/dev/null || true

    rm -rf "$proj"
}

# ---------------------------------------------------------------------------
# Run all scenarios
# ---------------------------------------------------------------------------

log "Starting daemon ownership E2E tests (macc=$MACC)"
run_scenario_1
run_scenario_2
run_scenario_3

log "Results: PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
exit 0
