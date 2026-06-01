#!/usr/bin/env bash
# Ch6 end-to-end validation (NOT user-facing). A Node stand-in for the browser
# exercises the same iii-browser-sdk mechanics the Vite UI uses: connect to the
# RBAC-gated worker-manager (:3110), create a link from the "browser", receive
# live clicks via the `stream` trigger, answer a server-initiated confirm.
# Also asserts the auth function rejects a bad token, that the allowlist blocks
# unallowed functions, and that the Vite UI compiles.
#
# Usage: ./e2e.sh   (run from this directory)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$HERE/linkly"
LOG="$(mktemp -t linkly-ch6-engine.XXXX.log)"
STANDIN_LOG="$(mktemp -t linkly-ch6-standin.XXXX.log)"
TRUSTED_PORT=49134
BROWSER_PORT=3110
HTTP_PORT=3111
STREAM_PORT=3112
CONSOLE_PORT=3113
ENGINE_PID=""
STANDIN_PID=""
fail=0

cleanup() {
  set +m   # suppress 'Terminated: 15' message when bash reaps the standin subshell
  [ -n "$STANDIN_PID" ] && kill "$STANDIN_PID" 2>/dev/null || true
  [ -n "$ENGINE_PID" ] && kill "$ENGINE_PID" 2>/dev/null || true
  for p in "$TRUSTED_PORT" "$BROWSER_PORT" "$CONSOLE_PORT" "$STREAM_PORT"; do
    lsof -ti "tcp:$p" 2>/dev/null | xargs kill -9 2>/dev/null || true
  done
  pkill -f 'iii-worker __watch-source' 2>/dev/null || true
  pkill -f 'bin/console' 2>/dev/null || true
  pkill -f 'bin/database' 2>/dev/null || true
  pkill -f 'watchfiles' 2>/dev/null || true
  pkill -f 'tsx watch' 2>/dev/null || true
  pkill -f 'standin.mjs' 2>/dev/null || true
}
trap cleanup EXIT

assert() { if printf '%s' "$3" | grep -qiF "$2"; then echo "  ok: $1"; else echo "  FAIL: $1 — expected '$2', got: $3"; fail=1; fi; }
wait_for_port() { for _ in $(seq 1 "$2"); do lsof -i "tcp:$1" >/dev/null 2>&1 && return 0; sleep 1; done; return 1; }
wait_for_log() { for _ in $(seq 1 "$2"); do grep -qF "$3" "$1" 2>/dev/null && return 0; sleep 1; done; return 1; }

cd "$PROJECT"
rm -rf data
iii > "$LOG" 2>&1 &
ENGINE_PID=$!
wait_for_port "$TRUSTED_PORT" 60 || { echo "trusted listener never opened"; cat "$LOG"; exit 1; }
wait_for_port "$BROWSER_PORT" 60 || { echo "browser listener never opened"; cat "$LOG"; exit 1; }
wait_for_port "$HTTP_PORT" 60 || { echo "iii-http never opened"; cat "$LOG"; exit 1; }
wait_for_port "$STREAM_PORT" 60 || { echo "iii-stream never opened"; cat "$LOG"; exit 1; }
iii worker restart link >/dev/null 2>&1 || true
sleep 3

# Sanity: local workers still reachable on the trusted port (RBAC must not gate them).
assert "trusted port still serves local workers" '"code": "health"' \
  "$(iii trigger link::create url=https://health.example code=health 2>&1)"

# --- Browser stand-in connects to the RBAC-gated port ---
( cd "$HERE/linkly/frontend" && node standin.mjs > "$STANDIN_LOG" 2>&1 ) &
STANDIN_PID=$!
wait_for_log "$STANDIN_LOG" 30 'EVENT created' || { echo "stand-in did not connect / create"; cat "$STANDIN_LOG"; exit 1; }
assert "browser-as-worker created a link via link::create" 'browser1' "$(cat "$STANDIN_LOG")"

# --- Live click stream ---
curl -s -o /dev/null "http://127.0.0.1:$HTTP_PORT/s/browser1"
wait_for_log "$STANDIN_LOG" 20 'EVENT click' || true
assert "browser received a click on the stream" 'EVENT click 1' "$(cat "$STANDIN_LOG")"

# --- Server-initiated confirm flow ---
del="$(iii trigger link::request_delete code=browser1 2>&1)"
assert "browser was asked to confirm" 'EVENT confirm-requested' "$(cat "$STANDIN_LOG")"
assert "server deletes only after the browser confirms" '"deleted": true' "$del"
assert "the link is gone after confirmed delete" '"url": null' \
  "$(iii trigger link::resolve code=browser1 2>&1)"

# --- Auth + allowlist enforcement on the browser listener ---
# Probe child uses `process.exit` aggressively because the SDK keeps reconnect
# timers alive after trigger() resolves/rejects; otherwise the subshell hangs.
probe='
import("iii-browser-sdk").then(async ({ registerWorker }) => {
  const [url, fn, payload] = process.argv.slice(1)
  const w = registerWorker(url)
  const bail = setTimeout(() => { console.log("TIMEOUT"); process.exit(0) }, 8000)
  try {
    await w.trigger({ function_id: fn, payload: JSON.parse(payload), timeoutMs: 4000 })
    console.log("ALLOWED")
  } catch (e) {
    console.log("REJECTED:", (e?.message || e).toString().slice(0, 200))
  }
  clearTimeout(bail)
  process.exit(0)
})'
bad="$(cd "$HERE/linkly/frontend" && node -e "$probe" "ws://localhost:$BROWSER_PORT?token=BOGUS" link::create '{"url":"x","code":"evil"}' 2>/dev/null || true)"
assert "bad token is rejected by the auth function" 'REJECTED' "$bad"
forbid="$(cd "$HERE/linkly/frontend" && node -e "$probe" "ws://localhost:$BROWSER_PORT?token=dev-token" database::execute '{"db":"primary","sql":"SELECT 1"}' 2>/dev/null || true)"
assert "expose_functions blocks unallowed functions" "not allowed" "$forbid"

# --- The Vite UI compiles (we don't drive a browser headlessly here) ---
if ( cd "$HERE/linkly/frontend" && npx tsc --noEmit && npx vite build ) >/dev/null 2>&1; then
  echo "  ok: Linkly UI typechecks and builds"
else
  echo "  FAIL: Linkly UI typecheck/build"
  fail=1
fi

echo
if [ "$fail" -eq 0 ]; then echo "Ch6 e2e: PASS"; else echo "Ch6 e2e: FAIL"; fi
exit "$fail"
