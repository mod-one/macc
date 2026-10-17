#!/usr/bin/env bash
set -euo pipefail

TOOL_ID="codex"
TOOL_LOG_PREFIX="codex"
# A failed resume may already have executed commands. Let the coordinator
# classify/retry it instead of silently executing the task a second time.
TOOL_RESUME_FALLBACK=false

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../shared/performer_lib.sh
source "$script_dir/../shared/performer_lib.sh"
