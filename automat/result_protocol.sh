#!/usr/bin/env bash
# Tool-agnostic terminal protocol projection. Keep transcripts for evidence;
# only unquoted, unindented final fields outside prompt echoes are candidates.
terminal_result_lines() {
  awk '
    /^MACC_PROMPT_BEGIN:/ { prompt=1; n=0; next }
    /^MACC_PROMPT_END:/ { prompt=0; next }
    prompt { next }
    /^[[:space:]]*```/ { fence=!fence; next }
    fence { next }
    /^(exec|tool|user|system|developer)$/ { transcript=1; next }
    /^(codex|assistant|final)$/ { transcript=0; n=0; boundary=0; next }
    transcript { next }
    /^MACC_TASK_(RESULT|RESULT_EXP|GATE_VERDICT|PRECONDITION):/ {
      # Templates may occur inside otherwise legitimate sentences.
      if (tolower($0) ~ /<(reason|condition|evidence|summary|explanation|result|kind|status|[^>]*\.\.\.)>/) next;
      if(boundary) {n=0; boundary=0}
      lines[++n]=$0; next
    }
    { if(n) boundary=1 }
    END {
      # A later final result cannot inherit a reason from an earlier attempt.
      start=1
      for(i=1;i<=n;i++) if(lines[i] ~ /^MACC_TASK_RESULT:/) {
        if(last) start=last+1;
        last=i
      }
      for(i=start;i<=n;i++) print lines[i]
    }
  ' "$1"
}
