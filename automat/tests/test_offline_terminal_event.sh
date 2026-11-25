#!/usr/bin/env bash
# Exercise the installed performer entry point with an unreachable coordinator.
set -euo pipefail

runner="$(cd "$(dirname "$0")/.." && pwd)/performer.sh"
project="$(mktemp -d)"
trap 'rm -rf "$project"' EXIT
worker="$project/.macc/worktree/worker-01"
mkdir -p "$worker/.macc"
git -C "$worker" init -q -b main
git -C "$worker" -c user.name=Fixture -c user.email=fixture@example.invalid commit -q --allow-empty -m base
printf '%s\n' '{"branch":"main","base_branch":"main"}' > "$worker/.macc/worktree.json"
printf '%s\n' '{}' > "$worker/.macc/tool.json"
printf '%s\n' '{"tasks":[{"id":"T","gate":{"kind":"human_approval"}}]}' > "$worker/worktree.prd.json"

# A human gate gives a deterministic native phase failure without invoking any tool.
result=0
MACC_COORDINATOR_IPC_ADDR=offline MACC_CLAIM_ID=claim COORDINATOR_RUN_ID=run COORDINATOR_EPOCH=3 \
  MACC_PERFORMER_GENERATION=generation timeout 20 bash "$runner" --repo "$project" \
  --worktree "$worker" --task-id T --tool fixture --registry "$project/registry.json" \
  --prd "$worker/worktree.prd.json" >"$project/output" 2>&1 || result=$?
shopt -s nullglob
events=("$project/.macc/state/performer-outbox/"*.json)
if [[ $result != 1 || ${#events[@]} != 1 ]]; then
  cat "$project/output" >&2
  exit 1
fi
jq -e '.type == "phase_result" and .status == "failed" and .payload.error_code == "E908"
  and .task_id == "T" and .phase == "dev" and .claim_id == "claim" and .coordinator_epoch == 3
  and .run_id == "run" and .performer_generation == "generation"' "${events[0]}" >/dev/null
echo 'PASS: offline terminal evidence retained under the project with its original assignment'
