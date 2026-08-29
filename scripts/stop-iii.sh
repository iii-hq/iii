#!/usr/bin/env bash
set -u

stop_pid_file() {
  local label="$1"
  local pid_file="$2"
  if [[ ! -f "$pid_file" ]]; then
    return
  fi

  local pid
  pid="$(cat "$pid_file")"
  local stopped=false
  # Signal descendants and the exact recorded child independently. Group
  # delivery succeeding does not prove that the recorded PID belongs to it.
  if kill -TERM -- "-$pid" 2>/dev/null; then
    stopped=true
  fi
  if kill -TERM "$pid" 2>/dev/null; then
    stopped=true
  fi
  if [[ "$stopped" == true ]]; then
    echo "Stopped $label (PID: $pid, file: $pid_file)"
  fi
  rm -f "$pid_file"
}

for pid_file in "$@"; do
  # Compose must receive SIGTERM first so it can stop its workers while the
  # external engine is still available.
  stop_pid_file "III Compose" "${pid_file}.compose.pid"
  stop_pid_file "III Engine" "$pid_file"
done
