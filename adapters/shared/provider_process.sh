#!/usr/bin/env bash
# Transport only. The native per-invocation supervisor owns descendant cleanup.
provider_pid=""
provider_capture_dir=""
provider_tee_pid=""
provider_quiesced=true

drain_provider_output() {
  if [[ -n "$provider_capture_dir" ]]; then
    if [[ -f "$provider_capture_dir/completion" ]] && [[ "$(cat -- "$provider_capture_dir/completion")" == quiescent-v1 ]]; then
      provider_quiesced=true
    else
      provider_quiesced=false
      # A crashed boundary must not hang output drain or finalize a checkpoint.
      if [[ -n "$provider_tee_pid" ]]; then kill "$provider_tee_pid" 2>/dev/null || true; fi
    fi
  fi
  if [[ -n "$provider_tee_pid" ]]; then
    wait "$provider_tee_pid" 2>/dev/null || true
    provider_tee_pid=""
  fi
  if [[ -n "$provider_capture_dir" ]]; then
    rm -f "$provider_capture_dir/output"
    rm -f "$provider_capture_dir/completion"
    rmdir "$provider_capture_dir" 2>/dev/null || true
    provider_capture_dir=""
  fi
}

stop_provider_process() {
  if [[ -n "$provider_pid" ]]; then
    # Never SIGKILL the supervisor: it must reap even tools which detached from
    # the provider's session. Checkpoint/release happen only after this wait.
    kill -TERM "$provider_pid" 2>/dev/null || true
    wait "$provider_pid" 2>/dev/null || true
    provider_pid=""
  fi
  drain_provider_output
}
