#!/usr/bin/env bash
set -euo pipefail

BINARY="./iii"
CONFIG=""
PORT=""
PID_FILE="/tmp/iii-engine.pid"
LOG_FILE="/tmp/iii-engine.log"
TIMEOUT=60
CLEANUP_GRACE_SECONDS="${III_START_CLEANUP_GRACE_SECONDS:-15}"
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

if [[ -z "$COMPOSE_FILE" ]]; then
  candidate="${CONFIG%.*}.worker-compose.yaml"
  if [[ -f "$candidate" ]]; then
    COMPOSE_FILE="$candidate"
  fi
fi

if [[ -n "$COMPOSE_FILE" && ! -f "$COMPOSE_FILE" ]]; then
  echo "Compose file not found: $COMPOSE_FILE" >&2
  exit 1
fi

COMPOSE_PID_FILE="${PID_FILE}.compose.pid"
engine_pid=""
compose_pid=""
ready=false
rm -f "$COMPOSE_PID_FILE"
: > "$LOG_FILE"

process_alive() {
  local pid="$1"
  kill -0 -- "-$pid" 2>/dev/null || kill -0 "$pid" 2>/dev/null
}

signal_process() {
  local signal="$1"
  local pid="$2"
  kill "-$signal" -- "-$pid" 2>/dev/null || kill "-$signal" "$pid" 2>/dev/null || true
}

terminate_pid() {
  local pid="$1"
  if [[ -z "$pid" ]]; then
    return
  fi
  if ! process_alive "$pid"; then
    return
  fi
  signal_process TERM "$pid"
  for _ in $(seq 1 "$CLEANUP_GRACE_SECONDS"); do
    if ! process_alive "$pid"; then
      break
    fi
    sleep 1
  done
  if process_alive "$pid"; then
    signal_process KILL "$pid"
  fi
  wait "$pid" 2>/dev/null || true
}

cleanup_failed_start() {
  if [[ "$ready" == true ]]; then
    return
  fi
  terminate_pid "$compose_pid"
  terminate_pid "$engine_pid"
  rm -f "$PID_FILE" "$COMPOSE_PID_FILE"
}
trap cleanup_failed_start EXIT

wait_for_engine() {
  local pid="$1"
  for _ in $(seq 1 "$TIMEOUT"); do
    if ! kill -0 "$pid" 2>/dev/null; then
      return 1
    fi
    if nc -z 127.0.0.1 "$PORT" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  return 1
}

wait_for_compose() {
  local pid="$1"
  for _ in $(seq 1 "$TIMEOUT"); do
    if ! kill -0 "$pid" 2>/dev/null; then
      return 1
    fi
    if grep -Eq '^up: (nothing to do|[0-9]+ of [0-9]+ changed)' "$LOG_FILE"; then
      return 0
    fi
    sleep 1
  done
  return 1
}

compose_args=(compose)
if [[ -n "$COMPOSE_NAMESPACE" ]]; then
  compose_args+=(--namespace "$COMPOSE_NAMESPACE")
fi

# An explicit --engine is owned by the caller. Start only Compose against it.
if [[ -n "$COMPOSE_FILE" && -n "$ENGINE_URL" ]]; then
  compose_args+=(--engine "$ENGINE_URL" --up --file "$COMPOSE_FILE")
  compose_state_dir="${III_COMPOSE_STATE_DIR:-${PID_FILE}.compose}"
  set -m
  III_COMPOSE_STATE_DIR="$compose_state_dir" \
    "$BINARY" "${compose_args[@]}" > "$LOG_FILE" 2>&1 &
  compose_pid=$!
  set +m
  echo "$compose_pid" > "$PID_FILE"
  echo "Waiting for III Compose workers..."
  if wait_for_compose "$compose_pid"; then
    echo "III Compose workers are ready (PID: $compose_pid)"
    ready=true
    exit 0
  fi
  echo "ERROR: III Compose failed to start within ${TIMEOUT}s"
  echo "--- Compose log ---"
  cat "$LOG_FILE"
  exit 1
fi

# Engine config and worker-compose are deliberately separate contracts.
set -m
"$BINARY" --config "$CONFIG" > "$LOG_FILE" 2>&1 &
engine_pid=$!
set +m
echo "$engine_pid" > "$PID_FILE"
echo "Waiting for III Engine on port $PORT..."
if ! wait_for_engine "$engine_pid"; then
  echo "ERROR: III Engine failed to start on port $PORT within ${TIMEOUT}s"
  echo "--- Engine log ---"
  cat "$LOG_FILE"
  exit 1
fi

if [[ -z "$COMPOSE_FILE" ]]; then
  echo "III Engine is ready on port $PORT (PID: $engine_pid)"
  ready=true
  exit 0
fi

compose_args+=(--engine "ws://127.0.0.1:${PORT}" --up --file "$COMPOSE_FILE")
compose_state_dir="${III_COMPOSE_STATE_DIR:-${PID_FILE}.compose}"
set -m
III_COMPOSE_STATE_DIR="$compose_state_dir" \
  "$BINARY" "${compose_args[@]}" >> "$LOG_FILE" 2>&1 &
compose_pid=$!
set +m
echo "$compose_pid" > "$COMPOSE_PID_FILE"
echo "Waiting for III Compose workers..."
if wait_for_compose "$compose_pid"; then
  echo "III Engine and Compose workers are ready (engine PID: $engine_pid, compose PID: $compose_pid)"
  ready=true
  exit 0
fi

echo "ERROR: III Compose failed to start within ${TIMEOUT}s"
echo "--- Engine and Compose log ---"
cat "$LOG_FILE"
exit 1
