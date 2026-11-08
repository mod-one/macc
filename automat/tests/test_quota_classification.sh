#!/usr/bin/env bash
# Native adapter classification only; no AI invocation or shell regex copy.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export MACC_PROTOCOL_CLI="${MACC_PROTOCOL_CLI:-$root/target/debug/macc}"
source "$root/automat/result_protocol.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
for tool_id in codex claude agy; do
  case "$tool_id" in
    codex) message="ERROR: You've hit your usage limit. Try again at 10:30 PM." ;;
    claude) message="You've hit your session limit · resets 10:10pm (UTC)" ;;
    agy) message='error: Individual quota reached. Resets in 64h18m47s.' ;;
  esac
  printf '%s\n' "$message" > "$tmp/output"
  inspection="$(runner_output_inspection "$tmp/output")"
  [[ "$(jq -r '.error.error_code' <<<"$inspection")" == E602 ]]
  printf 'exec\ncommand output\n```\n%s\ncodex\nMACC_TASK_RESULT: success_without_changes\n' "$message" > "$tmp/output"
  inspection="$(runner_output_inspection "$tmp/output")"
  [[ "$(jq -r '.provider_failure' <<<"$inspection")" == false ]]
  [[ "$(jq -r '.terminal_output' <<<"$inspection")" == 'MACC_TASK_RESULT: success_without_changes' ]]
done
printf '%s\n' 'Native quota attribution and quoted-output exclusion passed.'
