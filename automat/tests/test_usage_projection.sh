#!/usr/bin/env bash
# Exercise shared declarative output projection without invoking an AI CLI.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repo_root/adapters/shared/usage.sh"
fixture_dir="$(mktemp -d)"
trap 'rm -rf -- "$fixture_dir"; [[ -z "$usage_raw_capture" ]] || rm -f -- "$usage_raw_capture"' EXIT
output_capture="$fixture_dir/output"
usage_structured=true
usage_plan='{"profile":{"append_args":["--json"],"replace_values":{},"text_match":{"/role":"assistant"},"text":["/content"],"text_separator":"","session_id":"/session_id"}}'
cat > "$output_capture" <<'JSON'
{"type":"init","session_id":"native-123"}
{"role":"reasoning","content":"exclude-private-text"}
{"role":"assistant","content":"MACC_TASK_RESULT: success_"}
{"role":"assistant","content":"without_changes"}
{"type":"result","stats":{"input":17}}
JSON
cp "$output_capture" "$fixture_dir/original"
project_usage_output > "$fixture_dir/projected"
grep -qx 'MACC_TASK_RESULT: success_without_changes' "$fixture_dir/projected"
grep -qx 'session id: native-123' "$fixture_dir/projected"
! grep -q 'exclude-private-text' "$fixture_dir/projected"
cmp "$fixture_dir/original" "$usage_raw_capture"
argv=(--model future-model --yolo exec resume native-123)
apply_usage_args argv
[[ "${argv[*]}" == '--model future-model --yolo exec resume native-123 --json' ]]
apply_usage_args argv
[[ "${argv[*]}" == '--model future-model --yolo exec resume native-123 --json' ]]
rm -f -- "$usage_raw_capture"
usage_raw_capture=""
usage_plan='{"profile":{"append_args":[],"replace_values":{"--output-format":"json"},"text_match":{},"text":["/result"],"session_id":"/session_id"}}'
printf '%s\n' '{' '  "result": "MACC_TASK_RESULT: precondition_unmet",' '  "session_id": "native-456"' '}' > "$output_capture"
project_usage_output > "$fixture_dir/projected"
grep -qx 'MACC_TASK_RESULT: precondition_unmet' "$fixture_dir/projected"
argv=(--output-format text --model future-model --effort high --resume native-456)
apply_usage_args argv
[[ "${argv[*]}" == '--output-format json --model future-model --effort high --resume native-456' ]]
usage_plan='{"profile":{"append_args":["--output-format","stream-json"],"replace_values":{}}}'
apply_usage_args argv
apply_usage_args argv
[[ "${argv[*]}" == '--output-format stream-json --model future-model --effort high --resume native-456' ]]
echo 'PASS: usage projection preserves completion text, raw evidence, session metadata and execution arguments'
