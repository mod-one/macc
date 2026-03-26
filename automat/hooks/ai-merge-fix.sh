#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ai-merge-fix.sh --repo <path> --task-id <id> --branch <branch> --base-branch <branch>
    [--failure-step <step>] [--failure-reason <text>] [--conflicts <csv>]
    [--report-file <path>]
    --tool <tool_id> --worktree-path <path>
    [--task-title <text>] [--task-description <text>] [--task-objective <text>]
    [--merge-base <sha>] [--base-commits <text>] [--branch-commits <text>]
    [--base-diff-stat <text>] [--branch-diff-stat <text>] [--conflict-diff <text>]

Resolves a blocked local merge by invoking an AI performer with full
task context and three-way diff information.
EOF
}

REPO_DIR=""
TASK_ID=""
BRANCH=""
BASE_BRANCH=""
FAILURE_STEP=""
FAILURE_REASON=""
FAILURE_CONFLICTS=""
REPORT_FILE=""
# Task metadata passed directly from the coordinator (no registry lookup needed)
TASK_TOOL=""
TASK_WORKTREE=""
TASK_TITLE=""
TASK_DESCRIPTION=""
TASK_OBJECTIVE=""
# Three-way diff context computed by the coordinator
MERGE_BASE_SHA=""
BASE_COMMITS=""
BRANCH_COMMITS=""
BASE_DIFF_STAT=""
BRANCH_DIFF_STAT=""
CONFLICT_DIFF=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo) REPO_DIR="$2"; shift 2 ;;
    --task-id) TASK_ID="$2"; shift 2 ;;
    --branch) BRANCH="$2"; shift 2 ;;
    --base-branch) BASE_BRANCH="$2"; shift 2 ;;
    --failure-step) FAILURE_STEP="$2"; shift 2 ;;
    --failure-reason) FAILURE_REASON="$2"; shift 2 ;;
    --conflicts) FAILURE_CONFLICTS="$2"; shift 2 ;;
    --report-file) REPORT_FILE="$2"; shift 2 ;;
    --tool) TASK_TOOL="$2"; shift 2 ;;
    --worktree-path) TASK_WORKTREE="$2"; shift 2 ;;
    --task-title) TASK_TITLE="$2"; shift 2 ;;
    --task-description) TASK_DESCRIPTION="$2"; shift 2 ;;
    --task-objective) TASK_OBJECTIVE="$2"; shift 2 ;;
    --merge-base) MERGE_BASE_SHA="$2"; shift 2 ;;
    --base-commits) BASE_COMMITS="$2"; shift 2 ;;
    --branch-commits) BRANCH_COMMITS="$2"; shift 2 ;;
    --base-diff-stat) BASE_DIFF_STAT="$2"; shift 2 ;;
    --branch-diff-stat) BRANCH_DIFF_STAT="$2"; shift 2 ;;
    --conflict-diff) CONFLICT_DIFF="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $1" >&2; usage; exit 1 ;;
  esac
done

if [[ -z "$REPO_DIR" || -z "$TASK_ID" || -z "$BRANCH" || -z "$BASE_BRANCH" ]]; then
  echo "Error: missing required args (--repo, --task-id, --branch, --base-branch)" >&2
  usage
  exit 1
fi

command -v jq >/dev/null 2>&1 || { echo "Error: jq is required" >&2; exit 1; }

REPO_DIR="$(cd "$REPO_DIR" && pwd -P)"

# ---------------------------------------------------------------------------
# Resolve tool and runner
# ---------------------------------------------------------------------------
# The coordinator now passes --tool and --worktree-path directly, so we no
# longer need to read task_registry.json (which may not exist when the storage
# backend is SQLite).
#
# Legacy fallback: if --tool/--worktree-path were not provided, attempt the
# registry lookup for backward compatibility.
if [[ -z "$TASK_TOOL" || -z "$TASK_WORKTREE" ]]; then
  registry="${REPO_DIR}/.macc/automation/task/task_registry.json"
  if [[ -f "$registry" ]]; then
    TASK_TOOL="${TASK_TOOL:-$(jq -r --arg id "$TASK_ID" '(.tasks // [])[] | select(.id == $id) | (.tool // "")' "$registry")}"
    TASK_WORKTREE="${TASK_WORKTREE:-$(jq -r --arg id "$TASK_ID" '(.tasks // [])[] | select(.id == $id) | (.worktree.worktree_path // "")' "$registry")}"
  fi
fi
[[ -n "$TASK_TOOL" && -n "$TASK_WORKTREE" ]] || {
  echo "Error: could not resolve tool/worktree for task $TASK_ID (pass --tool and --worktree-path or ensure task_registry.json exists)" >&2
  exit 1
}

tool_json="${TASK_WORKTREE}/.macc/tool.json"
[[ -f "$tool_json" ]] || { echo "Error: tool.json not found: $tool_json" >&2; exit 1; }

runner="$(jq -r '.performer.runner // ""' "$tool_json")"
[[ -n "$runner" && "$runner" != "null" ]] || { echo "Error: performer.runner missing in tool.json" >&2; exit 1; }
if [[ "$runner" != /* ]]; then
  runner="${REPO_DIR}/${runner}"
fi
[[ -x "$runner" ]] || { echo "Error: runner not executable: $runner" >&2; exit 1; }

# ---------------------------------------------------------------------------
# Gather current merge state
# ---------------------------------------------------------------------------
if [[ -z "$FAILURE_STEP" ]]; then
  FAILURE_STEP="${MACC_MERGE_FAILURE_STEP:-unknown}"
fi
if [[ -z "$FAILURE_REASON" ]]; then
  FAILURE_REASON="${MACC_MERGE_FAILURE_REASON:-unknown}"
fi
if [[ -z "$FAILURE_CONFLICTS" ]]; then
  FAILURE_CONFLICTS="${MACC_MERGE_FAILURE_CONFLICTS:-}"
fi
if [[ -z "$REPORT_FILE" ]]; then
  REPORT_FILE="${MACC_MERGE_REPORT_FILE:-}"
fi
FAILURE_OUTPUT="${MACC_MERGE_FAILURE_OUTPUT:-}"
SUGGESTION="${MACC_MERGE_SUGGESTION:-git checkout ${BASE_BRANCH} && git merge ${BRANCH}}"

current_branch="$(git -C "$REPO_DIR" rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
status_short="$(git -C "$REPO_DIR" status --short 2>&1 || true)"
in_merge_state="no"
if git -C "$REPO_DIR" rev-parse -q --verify MERGE_HEAD >/dev/null 2>&1; then
  in_merge_state="yes"
fi
report_snippet=""
if [[ -n "$REPORT_FILE" && -f "$REPORT_FILE" ]]; then
  report_snippet="$(sed -n '1,200p' "$REPORT_FILE" 2>/dev/null || true)"
fi

# ---------------------------------------------------------------------------
# Build the enriched merge-fix prompt
# ---------------------------------------------------------------------------
prompt_file="$(mktemp)"
cat >"$prompt_file" <<EOF
You are a git merge-conflict resolution operator. Your job is to resolve a
blocked local merge so that both sides of the work are preserved correctly.

## Task context

The feature branch implements the following task:

- Task ID: ${TASK_ID}
- Title: ${TASK_TITLE:-unknown}
- Objective: ${TASK_OBJECTIVE:-unknown}
- Description: ${TASK_DESCRIPTION:-not provided}

## Merge context

- Repository: ${REPO_DIR}
- Base branch (merge target): ${BASE_BRANCH}
- Feature branch (to merge): ${BRANCH}
- Merge-base commit: ${MERGE_BASE_SHA:-unknown}
- Failure step: ${FAILURE_STEP}
- Failure reason: ${FAILURE_REASON}
- Conflict files: ${FAILURE_CONFLICTS:-none}
- Currently checked-out branch: ${current_branch:-unknown}
- Merge in progress (MERGE_HEAD): ${in_merge_state}
- Suggested command: ${SUGGESTION}

## Development progress on the base branch since fork

These commits were merged into the base branch after the feature branch forked.
They represent work from OTHER tasks that must be preserved:

\`\`\`text
${BASE_COMMITS:-no commits}
\`\`\`

Stat:
\`\`\`text
${BASE_DIFF_STAT:-no stat}
\`\`\`

## Development progress on the feature branch

These commits are from the task being merged. Their changes must also be
preserved:

\`\`\`text
${BRANCH_COMMITS:-no commits}
\`\`\`

Stat:
\`\`\`text
${BRANCH_DIFF_STAT:-no stat}
\`\`\`

## Per-file diff for conflicting files

Below is what each side changed in ONLY the conflicting files, relative to
the merge-base. Use this to understand the intent of each side:

${CONFLICT_DIFF:-No per-file diff available.}

## Current git status

\`\`\`text
${status_short}
\`\`\`

## Failure output

\`\`\`text
${FAILURE_OUTPUT}
\`\`\`

## Merge report excerpt

\`\`\`text
${report_snippet}
\`\`\`

## Instructions

1) Work ONLY on the merge of branch '${BRANCH}' into '${BASE_BRANCH}'.
2) Do NOT edit unrelated files. Do NOT run formatter/linter outside files
   touched by the merge.
3) If the merge is not already in progress, start it:
   git checkout ${BASE_BRANCH}
   git merge --no-ff ${BRANCH}
4) For each conflicting file, use the per-file diffs above to understand
   what EACH side intended. Keep both sides' changes — the base branch
   changes are from previously merged tasks and the feature branch changes
   are from the task being merged. Do not discard either side.
5) After resolving all conflicts, stage the files and complete the merge:
   git add <resolved_files>
   git commit --no-edit
6) Verify the result:
   git status --short
   git log --oneline -n 3
7) If you cannot resolve a conflict, output the precise blocking reason
   and stop. Do NOT leave conflict markers in files.
EOF

set +e
"$runner" \
  --prompt-file "$prompt_file" \
  --tool-json "$tool_json" \
  --repo "$REPO_DIR" \
  --worktree "$REPO_DIR" \
  --task-id "$TASK_ID" \
  --attempt 1 \
  --max-attempts 1
rc=$?
set -e

rm -f "$prompt_file"
exit "$rc"
