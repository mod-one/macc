#!/usr/bin/env bash
# Test the production functions without invoking any AI tool.
set -euo pipefail
performer="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/performer.sh"
eval "$(sed -n '/^detect_rate_limit()/,/^}/p; /^task_log_path()/,/^}/p' "$performer")"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
performer_log_dir="$tmp"
[[ "$(task_log_path SEC-API-004)" == "$tmp/SEC-API-004.md" ]]
for message in \
  'ERROR: You’ve hit your usage limit. Try again at Oct 4th, 2026 2:13 AM.' \
  'MACC_TOOL_LIMIT: quota_exhausted tool=codex' \
  "You've hit your session limit"; do
  printf '%s\n' "$message" > "$tmp/output"
  # grep -q over a pipe with pipefail used to lose matches on large output.
  for ((i=0; i<3000; i++)); do printf '%s\n' 'large unrelated output'; done >> "$tmp/output"
  LAST_ERROR_CODE=''
  detect_rate_limit "$tmp/output"
  [[ "$LAST_ERROR_CODE" == E602 ]]
done
printf '%s\n' '429 too many requests retry-after: 60' > "$tmp/output"
LAST_ERROR_CODE=''
detect_rate_limit "$tmp/output"
[[ "$LAST_ERROR_CODE" == E601 ]]
printf '%s\n' 'permission denied' > "$tmp/output"
LAST_ERROR_CODE=''
detect_rate_limit "$tmp/output"
[[ -z "$LAST_ERROR_CODE" ]]
printf '%s\n' 'Quota classification and log path tests passed.'
