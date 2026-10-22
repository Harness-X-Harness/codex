#!/usr/bin/env bash
# Observe the unchanged build command. The observer is always reaped on exit.
set -euo pipefail
python3 "$(dirname "$0")/observe_linux_build.py" "$$" &
observer=$!
cleanup() {
  build_status=$?
  trap - EXIT
  observer_status=0
  kill -TERM "$observer" 2>/dev/null || true # It may already have exited.
  wait "$observer" || observer_status=$?
  if (( observer_status != 0 && observer_status != 143 )); then
    echo 'grok-build-resources observer_failed' >&2
  fi
  exit "$build_status"
}
trap cleanup EXIT
"$@"
