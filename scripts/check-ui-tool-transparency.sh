#!/usr/bin/env bash
# scripts/check-ui-tool-transparency.sh

set -euo pipefail

# Get the directory of the script to resolve paths relative to the repo root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DENYLIST_FILE="$REPO_ROOT/scripts/ui-denylist.txt"
TARGET_DIRS=("tui/src" "cli/src")

if [ ! -f "$DENYLIST_FILE" ]; then
    echo "ERROR: Denylist file not found: $DENYLIST_FILE"
    exit 1
fi

mapfile -t TOKENS < <(grep -v '^#' "$DENYLIST_FILE" | grep -v '^$')
if [ "${#TOKENS[@]}" -eq 0 ]; then
    echo "Check skipped: Denylist is empty."
    exit 0
fi

escape_regex() {
    sed -E 's/[][(){}.^$+*?|\\-]/\\&/g'
}

PATTERN=""
for token in "${TOKENS[@]}"; do
    escaped="$(printf '%s' "$token" | escape_regex)"
    if [ -z "$PATTERN" ]; then
        PATTERN="$escaped"
    else
        PATTERN="$PATTERN|$escaped"
    fi
done

echo "Checking for forbidden tool strings in UI/client sources..."

cd "$REPO_ROOT"

if command -v cargo >/dev/null 2>&1; then
    if ! cargo test -p macc-core --test tool_name_guardrail --quiet -- --nocapture; then
        echo ""
        echo "ERROR: Tool-specific names found in UI/client source layers (cli/tui)."
        echo "Use generic IDs/capabilities and resolve concrete tools via ToolSpec + registry."
        exit 1
    fi
    echo "Check passed: source layers are tool-agnostic."
    exit 0
fi

FAILED=0
for DIR in "${TARGET_DIRS[@]}"; do
    if [ -d "$DIR" ]; then
        MATCHES=""
        if command -v rg >/dev/null 2>&1; then
            MATCHES=$(
                rg -n -i --pcre2 "$PATTERN" "$DIR" \
                    -g '*.rs' \
                    -g '!**/target/**' \
                    || true
            )
        elif command -v grep >/dev/null 2>&1; then
            MATCHES=$(
                grep -rnEI "$PATTERN" "$DIR" \
                    --include="*.rs" \
                    --exclude-dir="target" \
                    || true
            )
        fi

        if [ -n "$MATCHES" ]; then
            FILTERED_MATCHES=$(echo "$MATCHES" | grep -v "macc:allow-tool-name" | grep -v "tests_body.inc" || true)
            if [ -n "$FILTERED_MATCHES" ]; then
                echo "Forbidden strings found in $DIR/:
$FILTERED_MATCHES"
                FAILED=1
            fi
        fi
    fi
done

if [ $FAILED -eq 1 ]; then
    echo ""
    echo "ERROR: Tool-specific names found in UI/client source layers (cli/tui)."
    echo "Use generic IDs/capabilities and resolve concrete tools via ToolSpec + registry."
    exit 1
else
    echo "Check passed: source layers are tool-agnostic."
    exit 0
fi
