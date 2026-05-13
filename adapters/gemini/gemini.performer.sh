#!/usr/bin/env bash
set -euo pipefail

TOOL_ID="gemini"
TOOL_LOG_PREFIX="gemini"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../shared/performer_lib.sh
source "$script_dir/../shared/performer_lib.sh"
