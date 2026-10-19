#!/usr/bin/env bash
# Only transport, output formatting and adapter-declared argv changes live here.
usage_enabled=false
usage_structured=false
usage_finished=false
usage_plan='null'
usage_raw_capture=""
usage_max_output_bytes=1048576
prepare_usage() {
  [[ "$session_backend" == sqlite && -n "$sqlite_lease_key" ]] || return 0
  local plan invoke_command="$1"
  shift
  if ! plan="$( { if [[ $# -gt 0 ]]; then printf '%s\0' "$@"; fi; } | jq -Rsc --arg command "$invoke_command" --argjson key "$sqlite_lease_key" --arg phase "$checkpoint_phase" '$key+{phase:$phase,invocation:{command:$command,args:(split("\u0000")[:-1])}}' | session_service usage-begin)"; then
    echo 'Warning: usage capture could not start; preserving normal tool execution' >&2
    return 0
  fi
  usage_plan="$plan"
  usage_enabled="$(jq -r '.enabled' <<<"$plan")"
  usage_structured="$(jq -r '.structured' <<<"$plan")"
  usage_max_output_bytes="$(jq -r '.max_output_bytes' <<<"$plan")"
}
apply_usage_args() {
  [[ "$usage_structured" == true ]] || return 0
  local -n usage_argv="$1"
  local index flag replacement arg
  for ((index=0;index<${#usage_argv[@]};index++)); do
    flag="${usage_argv[$index]}"
    replacement="$(jq -r --arg flag "$flag" '.profile.replace_values[$flag] // empty' <<<"$usage_plan")"
    if [[ -n "$replacement" && $((index+1)) -lt ${#usage_argv[@]} ]]; then
      usage_argv[$((index+1))]="$replacement"; index=$((index+1))
    fi
  done
  local declared=() found usage_index
  while IFS= read -r arg; do declared+=("$arg"); done < <(jq -r '.profile.append_args[]?' <<<"$usage_plan")
  for ((index=0;index<${#declared[@]};index++)); do
    flag="${declared[$index]}"; replacement=""; found=false
    if [[ "$flag" == --output-format ]]; then
      index=$((index+1)); replacement="${declared[$index]}"
    fi
    for ((usage_index=0;usage_index<${#usage_argv[@]};usage_index++)); do
      if [[ "${usage_argv[$usage_index]}" == "$flag" ]]; then
        found=true
        if [[ -n "$replacement" && $((usage_index+1)) -lt ${#usage_argv[@]} ]]; then usage_argv[$((usage_index+1))]="$replacement"; fi
        break
      fi
    done
    if [[ "$found" == false ]]; then
      usage_argv+=("$flag")
      if [[ -n "$replacement" ]]; then usage_argv+=("$replacement"); fi
    fi
  done
}
# Add only declared assistant/result text and current-invocation session metadata.
# Original structured output remains in the task log for diagnostics.
project_usage_output() {
  [[ "$usage_structured" == true ]] || return 0
  if [[ "$(wc -c < "$output_capture")" -gt 16777216 ]]; then
    echo 'Error: machine output exceeds safe decoding bound; work preserved, completion not accepted' >&2
    return 1
  fi
  usage_raw_capture="$(mktemp "${TMPDIR:-/tmp}/macc-usage-output.XXXXXX")" || return 1
  cp -- "$output_capture" "$usage_raw_capture"
  local projected session
  projected="$(jq -Rrjs --argjson profile "$(jq -c .profile <<<"$usage_plan")" '
    def ptr($p): if $p=="" then . else reduce ($p|ltrimstr("/")|split("/")|map(gsub("~1";"/")|gsub("~0";"~")))[] as $key (.; if type=="array" then .[$key|tonumber] else .[$key] end) end;
    . as $raw | (try fromjson catch ($raw|split("\n")|map(fromjson?))) |
    (if type=="array" then .[] else . end) as $event |
    select(($profile.text_match // {} | to_entries | all(.[]; . as $entry | ($event | ptr($entry.key)) == $entry.value))) |
    $profile.text[]? as $p | $event | ptr($p) | select(type=="string") | . + ($profile.text_separator // "\n")
  ' "$output_capture" 2>/dev/null)" || { echo 'Error: structured output text could not be decoded; completion not accepted' >&2; return 1; }
  # Keep this raw capture for numeric parsing: appended text is never a stats source.
  printf '\n%s\n' "$projected" | tee -a "$output_capture"
  session="$(jq -Rrs --arg path "$(jq -r '.profile.session_id // empty' <<<"$usage_plan")" '
    def ptr($p): getpath($p|ltrimstr("/")|split("/")|map(gsub("~1";"/")|gsub("~0";"~")));
    . as $raw | (try fromjson catch ($raw|split("\n")|map(fromjson?))) |
    [ (if type=="array" then .[] else . end) | select($path!="") | ptr($path) | select(type=="string") ] | unique | if length==1 then .[0] else empty end
  ' "$usage_raw_capture" 2>/dev/null)" || true
  if [[ "$session" =~ ^[[:alnum:]-]+$ ]]; then printf 'session id: %s\n' "$session" | tee -a "$output_capture"; fi
}
finish_usage() {
  [[ "$usage_enabled" == true && "$usage_finished" == false && -n "$sqlite_lease_key" ]] || return 0
  usage_finished=true
  local omitted=false source_file="${usage_raw_capture:-$output_capture}"
  if [[ "$(wc -c < "$source_file")" -gt "$usage_max_output_bytes" ]]; then omitted=true; fi
  if ! { if [[ "$omitted" == true ]]; then printf ''; else cat -- "$source_file"; fi; } |
      jq -Rs --argjson key "$sqlite_lease_key" --arg outcome "$1" --argjson omitted "$omitted" '$key+{outcome:$outcome,raw:.,omitted:$omitted}' |
      session_service usage-end >/dev/null; then
    echo 'Warning: usage receipt was not saved; pending metrics remain unknown; task result preserved' >&2
  fi
}
