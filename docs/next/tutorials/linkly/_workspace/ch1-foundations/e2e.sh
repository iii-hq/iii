#!/usr/bin/env bash
# Ch1 end-to-end validation (NOT user-facing — validates the tutorial's code).
# Boots the engine for the linkly project, exercises link::create / link::resolve
# via `iii trigger`, then the HTTP edge (POST /links, GET /s/:code) via curl.
#
# Usage: ./e2e.sh   (run from this directory)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$HERE/linkly"
WORKER="$PROJECT/link"
LOG="$(mktemp -t linkly-ch1-engine.XXXX.log)"
SDK_PORT=49134
HTTP_PORT=3111
ENGINE_PID=""
fail=0

cleanup() {
  [ -n "$ENGINE_PID" ] && kill "$ENGINE_PID" 2>/dev/null || true
  lsof -ti "tcp:$SDK_PORT" 2>/dev/null | xargs kill -9 2>/dev/null || true
  pkill -f 'iii-worker __watch-source' 2>/dev/null || true
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

wait_for_port() { # wait_for_port <port> <seconds>
  for _ in $(seq 1 "$2"); do lsof -i "tcp:$1" >/dev/null 2>&1 && return 0; sleep 1; done
  return 1
}

cd "$PROJECT"

# config.yaml already wires link via a relative worker_path. Boot the engine.
iii > "$LOG" 2>&1 &
ENGINE_PID=$!

wait_for_port "$SDK_PORT" 30 || { echo "engine did not open $SDK_PORT"; cat "$LOG"; exit 1; }

# --- Domain functions via `iii trigger` ---
out="$(iii trigger link::create url=https://iii.dev code=iii 2>&1)"
assert "link::create returns custom code" '"code": "iii"' "$out"
out="$(iii trigger link::resolve code=iii 2>&1)"
assert "link::resolve returns the url" '"url": "https://iii.dev"' "$out"
out="$(iii trigger link::resolve code=nope 2>&1)"
assert "link::resolve returns null for unknown code" '"url": null' "$out"

# --- HTTP edge (iii-http is in config.yaml; restart link so triggers bind) ---
wait_for_port "$HTTP_PORT" 60 || { echo "iii-http did not open $HTTP_PORT"; cat "$LOG"; exit 1; }
iii worker restart link >/dev/null 2>&1 || true
sleep 2

out="$(curl -sS -i -X POST "http://127.0.0.1:$HTTP_PORT/links" \
  -H 'Content-Type: application/json' -d '{"url":"https://example.com","code":"demo"}' 2>&1)"
assert "POST /links -> 201" "201 Created" "$out"
assert "POST /links -> link json" '"code":"demo"' "$out"

out="$(curl -sS -i "http://127.0.0.1:$HTTP_PORT/s/demo" 2>&1)"
assert "GET /s/demo -> 302" "302 Found" "$out"
assert "GET /s/demo -> Location" "location: https://example.com" "$out"

out="$(curl -sS -i "http://127.0.0.1:$HTTP_PORT/s/missing" 2>&1)"
assert "GET /s/missing -> 404" "404 Not Found" "$out"

echo
if [ "$fail" -eq 0 ]; then echo "Ch1 e2e: PASS"; else echo "Ch1 e2e: FAIL"; fi
exit "$fail"
