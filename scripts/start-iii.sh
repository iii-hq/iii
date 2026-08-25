#!/usr/bin/env bash
set -euo pipefail

BINARY="./iii"
CONFIG=""
PORT=""
PID_FILE="/tmp/iii-engine.pid"
LOG_FILE="/tmp/iii-engine.log"
TIMEOUT=60
COMPOSE_FILE=""
COMPOSE_NAMESPACE=""
ENGINE_URL=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary)   BINARY="$2";   shift 2 ;;
    --config)   CONFIG="$2";   shift 2 ;;
    --port)     PORT="$2";     shift 2 ;;
    --pid-file) PID_FILE="$2"; shift 2 ;;
    --log-file) LOG_FILE="$2"; shift 2 ;;
    --timeout)  TIMEOUT="$2";  shift 2 ;;
    --compose-file) COMPOSE_FILE="$2"; shift 2 ;;
    --namespace) COMPOSE_NAMESPACE="$2"; shift 2 ;;
    --engine|--iii-url) ENGINE_URL="$2"; shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$CONFIG" || -z "$PORT" ]]; then
  echo "Usage: $0 --config <path> --port <port> [--binary <path>] [--pid-file <path>] [--log-file <path>] [--timeout <seconds>] [--compose-file <path>] [--namespace <name>] [--engine|--iii-url <ws-url>]" >&2
  exit 1
fi

if [[ ! -f "$CONFIG" ]]; then
  echo "Config file not found: $CONFIG" >&2
  exit 1
fi

if grep -Eq '^engine:[[:space:]]*' "$CONFIG"; then
  COMPOSE_FILE="$CONFIG"
elif [[ -z "$COMPOSE_FILE" ]]; then
  candidate="${CONFIG%.*}.worker-compose.yaml"
  if [[ -f "$candidate" ]]; then
    COMPOSE_FILE="$candidate"
  fi
fi

if [[ -n "$COMPOSE_FILE" ]]; then
  if [[ ! -f "$COMPOSE_FILE" ]]; then
    echo "Compose file not found: $COMPOSE_FILE" >&2
    exit 1
  fi
  if [[ -z "$COMPOSE_NAMESPACE" ]]; then
    config_name="$(basename "${CONFIG%.*}")"
    COMPOSE_NAMESPACE="sdk-${config_name//[^a-zA-Z0-9_-]/-}"
  fi
  if grep -Eq '^engine:[[:space:]]*' "$COMPOSE_FILE"; then
    if [[ -n "$ENGINE_URL" ]]; then
      echo "Managed Compose file $COMPOSE_FILE cannot be combined with --engine/--iii-url" >&2
      exit 1
    fi
    "$BINARY" compose \
      --namespace "$COMPOSE_NAMESPACE" \
      --up --file "$COMPOSE_FILE" > "$LOG_FILE" 2>&1 &
  else
    if [[ -z "$ENGINE_URL" ]]; then
      echo "Compose file $COMPOSE_FILE has no engine section; pass --engine or --iii-url" >&2
      exit 1
    fi
    "$BINARY" compose \
      --engine "$ENGINE_URL" \
      --namespace "$COMPOSE_NAMESPACE" \
      --up --file "$COMPOSE_FILE" > "$LOG_FILE" 2>&1 &
  fi
else
  "$BINARY" --config "$CONFIG" > "$LOG_FILE" 2>&1 &
fi
started_pid=$!
echo "$started_pid" > "$PID_FILE"

ready=false
cleanup_failed_start() {
  if [[ "$ready" == true ]]; then
    return
  fi

  if kill -0 "$started_pid" 2>/dev/null; then
    kill "$started_pid" 2>/dev/null || true
  fi
  wait "$started_pid" 2>/dev/null || true
  rm -f "$PID_FILE"
}
trap cleanup_failed_start EXIT

echo "Waiting for III Engine on port $PORT..."
for _ in $(seq 1 "$TIMEOUT"); do
  pid="$(cat "$PID_FILE")"
  if ! kill -0 "$pid" 2>/dev/null; then
    break
  fi

  if [[ -n "$COMPOSE_FILE" ]] && grep -Eq '^up: (nothing to do|[0-9]+ of [0-9]+ changed)' "$LOG_FILE"; then
    echo "III Engine and Compose workers are ready (PID: $pid)"
    ready=true
    exit 0
  fi

  if [[ -z "$COMPOSE_FILE" ]] && nc -z 127.0.0.1 "$PORT" 2>/dev/null; then
    echo "III Engine is ready on port $PORT (PID: $(cat "$PID_FILE"))"
    ready=true
    exit 0
  fi
  sleep 1
done

echo "ERROR: III Engine failed to start on port $PORT within ${TIMEOUT}s"
echo "--- Engine log ---"
cat "$LOG_FILE"
exit 1
