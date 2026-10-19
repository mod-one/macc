#!/usr/bin/env bash
# Known provider unavailability must retain the conversation and pending files.
set -euo pipefail
REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/repo/.macc/state" "$WORK/bin"
python3 - "$REPO_ROOT/registry/tools.d/claude.tool.yaml" "$WORK/repo/.macc/tool.json" <<'PY'
import json, sys, yaml
spec = yaml.safe_load(open(sys.argv[1]))
json.dump(spec, open(sys.argv[2], 'w'))
PY
printf 'Continue pending work\n' > "$WORK/prompt.txt"
printf 'pending work\n' > "$WORK/repo/pending.txt"
cat > "$WORK/bin/claude" <<'STUB'
#!/usr/bin/env bash
echo invoked >> "${CALL_LOG:?}"
echo "${PROVIDER_MESSAGE:?}" >&2
exit 1
STUB
chmod +x "$WORK/bin/claude"
export PATH="$WORK/bin:$PATH"
source "$REPO_ROOT/automat/tests/session_fixture.sh"
SID=01a0c3f1-c956-7ab2-a9d9-78a7e85ccdc1
for message in "You've hit your session limit · resets 10:10pm (UTC)" '429 too many requests' 'authentication failed'; do
  seed_owned_session "$WORK/repo" "$WORK/repo/.macc/tool.json" claude T-1 "$SID"
  : > "$WORK/calls"
  rc=0
  CALL_LOG="$WORK/calls" PROVIDER_MESSAGE="$message" timeout 30 \
    bash "$REPO_ROOT/adapters/claude/claude.performer.sh" \
    --prompt-file "$WORK/prompt.txt" --tool-json "$WORK/repo/.macc/tool.json" \
    --repo "$WORK/repo" --worktree "$WORK/repo" --task-id T-1 --session-id "$SID" \
    > "$WORK/output" 2>&1 || rc=$?
  [[ "$rc" == 1 && "$(wc -l < "$WORK/calls")" == 1 ]] || { cat "$WORK/output"; exit 1; }
  [[ "$(cat "$WORK/repo/pending.txt")" == 'pending work' ]]
  jq -e --arg sid "$SID" '.tools.claude.sessions[$sid].status == "available"' \
    "$WORK/repo/.macc/state/tool-sessions.json" > /dev/null
done
echo 'PASS: quota/rate/auth resume failures make one call and retain pending work/session'
