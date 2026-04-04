#!/usr/bin/env bash
for pid_file in "$@"; do
  if [ -f "$pid_file" ]; then
    pid="$(cat "$pid_file")"
    if kill "$pid" 2>/dev/null; then
      echo "Stopped III Engine (PID: $pid, file: $pid_file)"
    fi
    rm -f "$pid_file"
  fi
done
