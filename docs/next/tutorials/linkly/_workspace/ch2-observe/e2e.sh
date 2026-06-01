#!/usr/bin/env bash
# Ch2 end-to-end validation (NOT user-facing). Builds on Ch1 and checks the
# observability surface: structured logs, the cross-worker trace waterfall, and
# that the console serves. Boots the engine for the linkly project.
#
# Usage: ./e2e.sh   (run from this directory)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$HERE/linkly"
LOG="$(mktemp -t linkly-ch2-engine.XXXX.log)"
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

cd "$PROJECT"
iii > "$LOG" 2>&1 &
ENGINE_PID=$!
wait_for_port "$SDK_PORT" 40 || { echo "engine did not open $SDK_PORT"; cat "$LOG"; exit 1; }
wait_for_port "$HTTP_PORT" 60 || { echo "iii-http did not open $HTTP_PORT"; cat "$LOG"; exit 1; }
wait_for_port "$CONSOLE_PORT" 60 || { echo "console did not open $CONSOLE_PORT"; cat "$LOG"; exit 1; }

# Bind the http triggers (cold-boot race), then drive redirect traffic.
iii worker restart link >/dev/null 2>&1 || true
sleep 2
curl -s -X POST "http://127.0.0.1:$HTTP_PORT/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"iii"}' >/dev/null
for _ in $(seq 1 8); do curl -s -o /dev/null "http://127.0.0.1:$HTTP_PORT/s/iii"; done
curl -s -o /dev/null "http://127.0.0.1:$HTTP_PORT/s/missing"
sleep 1

# --- Structured logs (worker logs export on a delay; poll until both land) ---
logs=""
for _ in $(seq 1 15); do
  logs="$(iii trigger engine::logs::list --json '{"limit":1000}' 2>&1)"
  if printf '%s' "$logs" | grep -q 'link resolved' && printf '%s' "$logs" | grep -q 'link created'; then break; fi
  sleep 1
done
assert "logs capture link::resolve" "link resolved" "$logs"
assert "logs capture link::create" "link created" "$logs"

# --- Cross-worker trace waterfall ---
# Worker-side spans export on a delay, so a fresh redirect's trace can look
# truncated for a second or two. Poll recent redirect traces until one shows the
# full chain (down to link::resolve).
# TODO(re-verify after engine fix): the worker-span export delay is being addressed;
# once spans flush promptly, this poll can collapse to a single lookup.
tree=""
for _ in $(seq 1 20); do
  ids="$(iii trigger engine::traces::list --json '{"name":"GET /s/:code","limit":12}' 2>/dev/null \
    | grep '"trace_id"' | sed -E 's/.*"trace_id": *"([0-9a-f]+)".*/\1/' | sort -u || true)"
  for tid in $ids; do
    t="$(iii trigger engine::traces::tree --json "{\"trace_id\":\"$tid\"}" 2>/dev/null || true)"
    if printf '%s' "$t" | grep -q 'link::resolve'; then tree="$t"; break; fi
  done
  [ -n "$tree" ] && break
  sleep 1
done
assert "redirect trace fans out to http::redirect" "http::redirect" "$tree"
assert "redirect trace reaches link::resolve" "link::resolve" "$tree"

# --- Console serves ---
ccode="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$CONSOLE_PORT/")"
assert "console serves on :$CONSOLE_PORT" "200" "$ccode"
# Note: the console is a browser UI; this only checks it serves. Clicking through
# the trace explorer / function list is a manual step, not automated here.

# --- 302 redirect span has status=ok ---
# (Fixed in 0.16.x; previously tagged "error".) Pull recent GET /s/:code spans
# and assert at least one with http=302 reports span status "ok".
redirect_ok="$(iii trigger engine::traces::list --json '{"name":"GET /s/:code","limit":20}' 2>/dev/null \
  | jq -r '[.spans[] | (.attributes | map({key: .[0], value: .[1]}) | from_entries | ."http.response.status_code") as $c | select($c == 302) | .status] | first // "missing"')"
assert "302 redirect span has status=ok" "ok" "$redirect_ok"

# --- TODO(re-verify after engine fix) ---
# engine::traces::list sort_by=duration_ms is currently inverted/unsorted.
# Once fixed, add an assertion that `sort_order: "desc"` returns spans whose
# durations are non-increasing. See ./verify/observe.py for the live repro.

echo
if [ "$fail" -eq 0 ]; then echo "Ch2 e2e: PASS"; else echo "Ch2 e2e: FAIL"; fi
exit "$fail"
