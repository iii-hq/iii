#!/usr/bin/env bash
# Ch4 end-to-end validation (NOT user-facing). Clicks go through the `clicks`
# queue (redirect stays fast); link.created fans out to the Python analytics
# worker; link.updated triggers cache invalidation. Boots the engine cold.
#
# Usage: ./e2e.sh   (run from this directory)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$HERE/linkly"
LOG="$(mktemp -t linkly-ch4-engine.XXXX.log)"
SDK_PORT=49134
HTTP_PORT=3111
CONSOLE_PORT=3113
ENGINE_PID=""
fail=0

cleanup() {
  [ -n "$ENGINE_PID" ] && kill "$ENGINE_PID" 2>/dev/null || true
  lsof -ti "tcp:$SDK_PORT" 2>/dev/null | xargs kill -9 2>/dev/null || true
  lsof -ti "tcp:$CONSOLE_PORT" 2>/dev/null | xargs kill -9 2>/dev/null || true
  pkill -f 'iii-worker __watch-source' 2>/dev/null || true
  pkill -f 'bin/console' 2>/dev/null || true
  pkill -f 'bin/database' 2>/dev/null || true
  pkill -f 'watchfiles' 2>/dev/null || true
  pkill -f 'tsx watch' 2>/dev/null || true
}
trap cleanup EXIT

assert() { # assert <label> <expected-substring> <actual>
  if printf '%s' "$3" | grep -qiF "$2"; then echo "  ok: $1"; else
    echo "  FAIL: $1 — expected '$2', got: $3"; fail=1; fi
}
wait_for_port() { for _ in $(seq 1 "$2"); do lsof -i "tcp:$1" >/dev/null 2>&1 && return 0; sleep 1; done; return 1; }
q() { iii trigger "$1" --json "$2" 2>&1; }
# poll_assert <label> <expected> <fn> <json> : retry the query until it matches (~15s)
poll_assert() {
  local label="$1" want="$2" fn="$3" body="$4" out=""
  for _ in $(seq 1 15); do
    out="$(q "$fn" "$body" || true)"
    printf '%s' "$out" | grep -qF "$want" && { echo "  ok: $label"; return; }
    sleep 1
  done
  echo "  FAIL: $label — expected '$want', got: $out"; fail=1
}

cd "$PROJECT"
rm -rf data
iii > "$LOG" 2>&1 &
ENGINE_PID=$!
wait_for_port "$SDK_PORT" 40 || { echo "engine did not open $SDK_PORT"; cat "$LOG"; exit 1; }
wait_for_port "$HTTP_PORT" 90 || { echo "iii-http did not open $HTTP_PORT"; cat "$LOG"; exit 1; }

# Bind http + subscribe triggers (cold-boot race) and ensure the schema.
iii worker restart link >/dev/null 2>&1 || true
iii worker restart analytics >/dev/null 2>&1 || true
sleep 3

# --- Create fans out to the Python analytics subscriber (link.created) ---
created="$(curl -s -X POST "http://127.0.0.1:$HTTP_PORT/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"iii"}')"
assert "POST /links returns the link" '"code":"iii"' "$created"
poll_assert "analytics counted the new link (link.created → python)" '"count": 1' \
  database::query '{"db":"analytics","sql":"SELECT count FROM daily_link_counts"}'

# --- Redirect enqueues the click; the consumer writes it asynchronously ---
rc="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HTTP_PORT/s/iii")"
assert "GET /s/iii redirects (302)" "302" "$rc"
poll_assert "click was recorded off the queue" '"n": 1' \
  database::query '{"db":"primary","sql":"SELECT COUNT(*) AS n FROM clicks"}'

# --- Update publishes link.updated; the subscriber refreshes the cache ---
updated="$(curl -s -X PUT "http://127.0.0.1:$HTTP_PORT/links/iii" -H 'Content-Type: application/json' \
  -d '{"url":"https://changed.example"}')"
assert "PUT /links/iii returns the new target" "https://changed.example" "$updated"
poll_assert "cache invalidation refreshed the state cache" "https://changed.example" \
  state::get '{"scope":"links","key":"iii"}'

echo
if [ "$fail" -eq 0 ]; then echo "Ch4 e2e: PASS"; else echo "Ch4 e2e: FAIL"; fi
exit "$fail"
