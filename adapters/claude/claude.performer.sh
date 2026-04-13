#!/usr/bin/env bash
set -euo pipefail

TOOL_ID="claude"
TOOL_LOG_PREFIX="claude"

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../shared/performer_lib.sh
source "$script_dir/../shared/performer_lib.sh"
