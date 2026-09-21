#!/usr/bin/env bash
# Codex receives its reasoning effort explicitly on every invocation, as the
# official Codex SDK does: `-c model_reasoning_effort="<effort>"`, while
# effort_config keeps writing the same value to .codex/config.toml.
#
# Drives the REAL adapters/shared/performer_lib.sh with the REAL
# registry/tools.d/codex.tool.yaml and a stub `codex` that records argv.
#
# Prerequisites: jq, python3 (with PyYAML)
set -euo pipefail

PASS=0; FAIL=0
log()  { printf '[test_codex_effort_args] %s\n' "$*"; }
pass() { log "PASS: $*"; PASS=$((PASS + 1)); }
fail() { log "FAIL: $*"; FAIL=$((FAIL + 1)); }
for c in jq python3; do command -v "$c" >/dev/null || { log "SKIP: $c missing"; exit 0; }; done
python3 -c 'import yaml' 2>/dev/null || { log "SKIP: PyYAML missing"; exit 0; }

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
SPEC="$REPO_ROOT/registry/tools.d/codex.tool.yaml"
RUNNER="$REPO_ROOT/adapters/codex/codex.performer.sh"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# tool.json from the shipped spec, with {model}/{effort} resolved as
# core/src/worktree.rs does from the configured model and reasoning effort.
mkdir -p "$WORK/repo/.macc/state"
python3 - "$SPEC" "$WORK/repo/.macc/tool.json" <<'PY'
import json, sys, yaml
spec = yaml.safe_load(open(sys.argv[1]))
def sub(o):
    if isinstance(o, list): return [sub(x) for x in o]
    if isinstance(o, dict): return {k: sub(v) for k, v in o.items()}
    if isinstance(o, str): return o.replace("{model}", "gpt-5.4").replace("{effort}", "medium")
    return o
spec["performer"] = sub(spec["performer"])
json.dump(spec, open(sys.argv[2], "w"))
PY
echo "do the thing" > "$WORK/prompt.txt"

mkdir -p "$WORK/bin"
cat > "$WORK/bin/codex" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$@" >> "${ARGV_LOG:?}"
echo "session id: 01a0c3f1-c956-7ab2-a9d9-78a7e85ccdc1"
echo "MACC_TASK_RESULT: success"
STUB
chmod +x "$WORK/bin/codex"; export PATH="$WORK/bin:$PATH"

run() {  # $1 = argv log; rest = extra runner args. MACC_MODEL_TIER from env.
  local log="$1"; shift; : > "$log"
  ( cd "$WORK/repo" && ARGV_LOG="$log" timeout 60 "$RUNNER" \
      --prompt-file "$WORK/prompt.txt" --tool-json "$WORK/repo/.macc/tool.json" \
      --repo "$WORK/repo" --worktree "$WORK/repo" --task-id T-1 "$@" >/dev/null 2>&1 ) || true
}
# Exact argv adjacency: "<a>" immediately followed by "<b>".
pair() { awk -v a="$1" -v b="$2" 'prev==a && $0==b {f=1} {prev=$0} END{exit !f}' "$3"; }
argv() { tr '\n' ' ' < "$1"; }

# 1. No routing tier: configured effort goes on the command line.
L="$WORK/a.log"; MACC_MODEL_TIER="" run "$L" --attempt 1 --max-attempts 1
pair "-c" 'model_reasoning_effort="medium"' "$L" \
  && pass "configured effort passed as -c model_reasoning_effort=\"medium\"" \
  || fail "configured effort missing; argv: $(argv "$L")"
pair "--model" "gpt-5.4" "$L" && pass "configured model kept" || fail "model; argv: $(argv "$L")"
grep -qxF -- "'model_reasoning_effort=\"medium\"'" "$L" \
  && fail "argument carries literal single quotes (shell quoting leaked into argv)" \
  || pass "no literal single quotes reach codex"

# 2. Routing tier `heavy`: tier model and tier effort, and config.toml agrees.
rm -f "$WORK/repo/.codex/config.toml"
L="$WORK/b.log"; MACC_MODEL_TIER=heavy MACC_MODEL_ROUTING_MODE=auto run "$L" --attempt 1 --max-attempts 1
pair "--model" "gpt-5.5" "$L" && pass "tier model applied" || fail "tier model; argv: $(argv "$L")"
pair "-c" 'model_reasoning_effort="high"' "$L" \
  && pass "tier effort applied on the command line" || fail "tier effort; argv: $(argv "$L")"
grep -qF 'model_reasoning_effort="medium"' "$L" && fail "stale configured effort still present" || pass "no stale effort left"
grep -qE '^model_reasoning_effort = "high"' "$WORK/repo/.codex/config.toml" 2>/dev/null \
  && pass "effort_config still writes .codex/config.toml" \
  || fail "effort_config not written; config.toml: $(cat "$WORK/repo/.codex/config.toml" 2>/dev/null)"

# 3. Resume with a session id under a tier: exec resume <id> keeps the effort.
L="$WORK/c.log"; MACC_MODEL_TIER=heavy MACC_MODEL_ROUTING_MODE=auto \
  run "$L" --attempt 2 --max-attempts 3 --session-id 01a0c3f1-c956-7ab2-a9d9-78a7e85ccdc1
pair "resume" "01a0c3f1-c956-7ab2-a9d9-78a7e85ccdc1" "$L" && pass "exec resume <session_id>" || fail "resume; argv: $(argv "$L")"
pair "-c" 'model_reasoning_effort="high"' "$L" && pass "resume carries the tier effort" || fail "resume effort; argv: $(argv "$L")"
[[ "$(grep -cxF -- '-c' "$L")" == "1" ]] && pass "effort passed once (no duplicate -c)" || fail "duplicate -c; argv: $(argv "$L")"

log "Passed: $PASS  Failed: $FAIL"; [[ $FAIL -eq 0 ]]
