#!/usr/bin/env bash
set -euo pipefail

BINARY="./iii"
CONFIG=""
PORT=""
PID_FILE="/tmp/iii-engine.pid"
LOG_FILE="/tmp/iii-engine.log"
TIMEOUT=60

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary)   BINARY="$2";   shift 2 ;;
    --config)   CONFIG="$2";   shift 2 ;;
    --port)     PORT="$2";     shift 2 ;;
    --pid-file) PID_FILE="$2"; shift 2 ;;
    --log-file) LOG_FILE="$2"; shift 2 ;;
    --timeout)  TIMEOUT="$2";  shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$CONFIG" || -z "$PORT" ]]; then
  echo "Usage: $0 --config <path> --port <port> [--binary <path>] [--pid-file <path>] [--log-file <path>] [--timeout <seconds>]" >&2
  exit 1
fi

"$BINARY" --config "$CONFIG" > "$LOG_FILE" 2>&1 &
echo $! > "$PID_FILE"

echo "Waiting for III Engine on port $PORT..."
for i in $(seq 1 "$TIMEOUT"); do
  if nc -z 127.0.0.1 "$PORT" 2>/dev/null; then
    echo "III Engine is ready on port $PORT (PID: $(cat "$PID_FILE"))"
    exit 0
  fi
  sleep 1
done

echo "ERROR: III Engine failed to start on port $PORT within ${TIMEOUT}s"
echo "--- Engine log ---"
cat "$LOG_FILE"
exit 1
