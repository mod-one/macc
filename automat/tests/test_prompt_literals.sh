#!/usr/bin/env bash
# Execute the real prompt builder with intercepted commands, no AI or coordinator.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT
worktree="$tmp"
prd="$tmp/task with spaces.json"
resume_attempt=0
# A prompt must never execute any of these literal instruction names.
gate() { touch "$tmp/command-executed"; }
verdict() { touch "$tmp/command-executed"; }
macc() { touch "$tmp/command-executed"; }
eval "$(sed -n '/^build_prompt()/,/^}/p' "$root/automat/performer.sh")"
build_prompt '{"id":"A","gate":{"kind":"human_approval"}}' A 'Literal protocol' > "$tmp/prompt" 2> "$tmp/stderr"
[[ ! -s "$tmp/stderr" && ! -e "$tmp/command-executed" ]]
grep -Fq '`gate`' "$tmp/prompt"
grep -Fq '`macc coordinator approve`' "$tmp/prompt"
grep -Fq "Task file: $prd" "$tmp/prompt"
grep -Fq 'Now implement the task !' "$tmp/prompt"
echo 'PASS: prompt instructions and task paths are literal data'
