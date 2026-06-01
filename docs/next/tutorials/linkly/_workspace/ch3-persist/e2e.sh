#!/usr/bin/env bash
# Ch3 end-to-end validation (NOT user-facing). Links now persist: the database
# holds the durable record, iii-state is the hot lookup cache, and clicks are
# recorded. Boots the engine for the linkly project from an empty data dir.
#
# Usage: ./e2e.sh   (run from this directory)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$HERE/linkly"
LOG="$(mktemp -t linkly-ch3-engine.XXXX.log)"
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
}
trap cleanup EXIT

assert() { # assert <label> <expected-substring> <actual>
  if printf '%s' "$3" | grep -qiF "$2"; then
    echo "  ok: $1"
  else
    echo "  FAIL: $1 — expected to contain '$2', got: $3"
    fail=1
  fi
}

wait_for_port() { for _ in $(seq 1 "$2"); do lsof -i "tcp:$1" >/dev/null 2>&1 && return 0; sleep 1; done; return 1; }
q() { iii trigger "$1" --json "$2" 2>&1; }

cd "$PROJECT"
rm -rf data   # start from an empty database + state store
iii > "$LOG" 2>&1 &
ENGINE_PID=$!
wait_for_port "$SDK_PORT" 40 || { echo "engine did not open $SDK_PORT"; cat "$LOG"; exit 1; }
wait_for_port "$HTTP_PORT" 60 || { echo "iii-http did not open $HTTP_PORT"; cat "$LOG"; exit 1; }

# Bind the http triggers and (re)run ensureSchema with the database ready.
iii worker restart link >/dev/null 2>&1 || true
sleep 3

# --- Create writes to both the durable table and the hot cache ---
created="$(curl -s -X POST "http://127.0.0.1:$HTTP_PORT/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"iii"}')"
assert "POST /links returns the link" '"code":"iii"' "$created"
assert "link is in the database" "https://iii.dev" \
  "$(q database::query '{"db":"primary","sql":"SELECT url FROM links WHERE code = ?","params":["iii"]}')"
assert "link is in the state cache" "https://iii.dev" \
  "$(q state::get '{"scope":"links","key":"iii"}')"

# --- A redirect records a click ---
rc="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HTTP_PORT/s/iii")"
assert "GET /s/iii redirects (302)" "302" "$rc"
sleep 1
assert "a click row was recorded" '"n": 1' \
  "$(q database::query '{"db":"primary","sql":"SELECT COUNT(*) AS n FROM clicks"}')"

# --- A DB-only link resolves via fallback and warms the cache ---
q database::execute '{"db":"primary","sql":"INSERT INTO links (code,url,created_at) VALUES (?,?,?)","params":["dbonly","https://example.org","2026-01-01T00:00:00Z"]}' >/dev/null
assert "resolve falls back to the database" "https://example.org" \
  "$(q link::resolve '{"code":"dbonly"}')"
assert "cache is warmed after the DB hit" "https://example.org" \
  "$(q state::get '{"scope":"links","key":"dbonly"}')"

# --- Links survive a worker restart (no longer in worker memory) ---
iii worker restart link >/dev/null 2>&1 || true
sleep 2
assert "links survive a worker restart" "https://iii.dev" \
  "$(q link::resolve '{"code":"iii"}')"

echo
if [ "$fail" -eq 0 ]; then echo "Ch3 e2e: PASS"; else echo "Ch3 e2e: FAIL"; fi
exit "$fail"
