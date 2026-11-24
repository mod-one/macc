#!/usr/bin/env bash
set -euo pipefail

TOOL_ID="codex"
TOOL_LOG_PREFIX="codex"
# A failed resume may already have executed commands. Let the coordinator
# classify/retry it instead of silently executing the task a second time.
TOOL_RESUME_FALLBACK=false
TOOL_FINAL_MESSAGE_FLAG=--output-last-message
TOOL_EVENT_STREAM_FLAG=--json
TOOL_EVENT_SESSION_POINTER=/thread_id

# A fresh session acknowledges the checkpoint before work. Codex emits this
# as a completed assistant message even when a later message carries the result.
provider_checkpoint_ack() {
  jq -Rn -e --arg ack "$2" '
    any(inputs | fromjson? | objects | select(.type == "item.completed")
        | .item | objects | select(.type == "agent_message") | .text | strings;
        split("\n")[0] == $ack)
  ' "$1" >/dev/null
}

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../shared/performer_lib.sh
source "$script_dir/../shared/performer_lib.sh"
