#!/usr/bin/env bash
# Ch5 end-to-end validation (NOT user-facing). iii-cron sweeps expired links and
# builds a daily top-links digest; iii-stream carries live clicks. Boots cold.
#
# Usage: ./e2e.sh   (run from this directory)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$HERE/linkly"
LOG="$(mktemp -t linkly-ch5-engine.XXXX.log)"
SDK_PORT=49134
HTTP_PORT=3111
STREAM_PORT=3112
CONSOLE_PORT=3113
ENGINE_PID=""
fail=0

cleanup() {
  [ -n "$ENGINE_PID" ] && kill "$ENGINE_PID" 2>/dev/null || true
  for p in "$SDK_PORT" "$STREAM_PORT" "$CONSOLE_PORT"; do lsof -ti "tcp:$p" 2>/dev/null | xargs kill -9 2>/dev/null || true; done
  pkill -f 'iii-worker __watch-source' 2>/dev/null || true
  pkill -f 'bin/console' 2>/dev/null || true
  pkill -f 'bin/database' 2>/dev/null || true
  pkill -f 'watchfiles' 2>/dev/null || true
  pkill -f 'tsx watch' 2>/dev/null || true
}
trap cleanup EXIT

assert() { if printf '%s' "$3" | grep -qiF "$2"; then echo "  ok: $1"; else echo "  FAIL: $1 — expected '$2', got: $3"; fail=1; fi; }
wait_for_port() { for _ in $(seq 1 "$2"); do lsof -i "tcp:$1" >/dev/null 2>&1 && return 0; sleep 1; done; return 1; }
q() { iii trigger "$1" --json "$2" 2>&1; }
poll_assert() {
  local label="$1" want="$2" fn="$3" body="$4" out=""
  for _ in $(seq 1 20); do out="$(q "$fn" "$body" || true)"; printf '%s' "$out" | grep -qF "$want" && { echo "  ok: $label"; return; }; sleep 1; done
  echo "  FAIL: $label — expected '$want', got: $out"; fail=1
}

cd "$PROJECT"
rm -rf data
iii > "$LOG" 2>&1 &
ENGINE_PID=$!
wait_for_port "$SDK_PORT" 40 || { echo "engine did not open $SDK_PORT"; cat "$LOG"; exit 1; }
wait_for_port "$HTTP_PORT" 90 || { echo "iii-http did not open $HTTP_PORT"; cat "$LOG"; exit 1; }
wait_for_port "$STREAM_PORT" 90 || { echo "iii-stream did not open $STREAM_PORT"; cat "$LOG"; exit 1; }
iii worker restart link >/dev/null 2>&1 || true
sleep 3

# --- Cron: sweep expired links (sweep is a normal function, callable directly) ---
q link::create '{"url":"https://old.example","code":"old","expires_at":"2000-01-01T00:00:00Z"}' >/dev/null
q link::create '{"url":"https://keep.example","code":"keep"}' >/dev/null
assert "sweep removes the expired link" '"swept": 1' "$(q link::sweep_expired '{}')"
assert "expired link no longer resolves" '"url": null' "$(q link::resolve '{"code":"old"}')"
assert "unexpired link still resolves" "https://keep.example" "$(q link::resolve '{"code":"keep"}')"

# --- Stream + digest: redirects produce clicks (async via the queue) ---
for _ in $(seq 1 3); do curl -s -o /dev/null "http://127.0.0.1:$HTTP_PORT/s/keep"; done
poll_assert "clicks reach the live stream" '"keep"' stream::list '{"stream_name":"clicks","group_id":"all"}'

# --- Cron: daily digest of most-clicked links ---
poll_assert "daily digest ranks the clicked link" '"code": "keep"' link::daily_digest '{}'

echo
if [ "$fail" -eq 0 ]; then echo "Ch5 e2e: PASS"; else echo "Ch5 e2e: FAIL"; fi
exit "$fail"
