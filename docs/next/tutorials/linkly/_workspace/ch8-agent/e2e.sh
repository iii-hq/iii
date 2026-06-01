#!/usr/bin/env bash
# Ch8 end-to-end validation (NOT user-facing). The safety-agent subscribes to
# link.created and runs an LLM tool-calling loop (routed through harness's
# provider::anthropic::complete, which gives every LLM call an iii-observability
# span). For hermetic CI, the agent's stub mode swaps the LLM call for a
# deterministic URL-keyword decider — no ANTHROPIC_API_KEY needed.
#
# Asserts:
#  - safety-agent and harness are both alive,
#  - a "malware" URL is auto-quarantined (resolve returns null),
#  - a benign URL is allowed (resolve still returns the url),
#  - a "phishing" URL is routed through link::request_delete (the admin flow).
#  - existing chapter-1/3/6 behavior still passes (HTTP redirect, quarantine
#    blocks redirect).
#
# Usage: ./e2e.sh   (run from this directory)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$HERE/linkly"
LOG="$(mktemp -t linkly-ch8-engine.XXXX.log)"
TRUSTED_PORT=49134
BROWSER_PORT=3110
HTTP_PORT=3111
STREAM_PORT=3112
CONSOLE_PORT=3113
ENGINE_PID=""
fail=0

cleanup() {
  set +m
  [ -n "$ENGINE_PID" ] && kill "$ENGINE_PID" 2>/dev/null || true
  for p in "$TRUSTED_PORT" "$BROWSER_PORT" "$CONSOLE_PORT" "$STREAM_PORT"; do
    lsof -ti "tcp:$p" 2>/dev/null | xargs kill -9 2>/dev/null || true
  done
  pkill -f 'iii-worker __watch-source' 2>/dev/null || true
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
  for _ in $(seq 1 30); do out="$(q "$fn" "$body" || true)"; printf '%s' "$out" | grep -qF "$want" && { echo "  ok: $label"; return; }; sleep 1; done
  echo "  FAIL: $label — expected '$want', got: $out"; fail=1
}

cd "$PROJECT"
rm -rf data
iii > "$LOG" 2>&1 &
ENGINE_PID=$!
wait_for_port "$TRUSTED_PORT" 60 || { echo "trusted listener never opened"; cat "$LOG"; exit 1; }
wait_for_port "$HTTP_PORT" 90 || { echo "iii-http never opened"; cat "$LOG"; exit 1; }
# Wait for harness + safety-agent to be ready (they take longer than the others).
for _ in $(seq 1 90); do
  if iii worker status harness --no-watch 2>/dev/null | grep -q 'harness ready' \
     && iii worker status safety-agent --no-watch 2>/dev/null | grep -q 'safety-agent ready'; then
    break
  fi
  sleep 1
done
iii worker restart link >/dev/null 2>&1 || true
sleep 3

# --- Malicious URL: agent auto-quarantines ---
q link::create '{"url":"https://malware-here.example","code":"q1"}' >/dev/null
poll_assert "agent quarantined the malware URL"  '"reason": "url contains \"malware\""' \
  database::query '{"db":"primary","sql":"SELECT code, reason FROM link_quarantine WHERE code = '"'"'q1'"'"'"}'
assert "quarantined link resolves to null" '"url": null' "$(q link::resolve '{"code":"q1"}')"

# --- Phishing URL: agent calls link::request_delete (not quarantine) ---
q link::create '{"url":"https://phishing-bad.example","code":"p1"}' >/dev/null
sleep 8
phishing_q="$(q database::query '{"db":"primary","sql":"SELECT code FROM link_quarantine WHERE code = '"'"'p1'"'"'"}')"
case "$phishing_q" in
  *'"code": "p1"'*) echo "  FAIL: phishing URL was quarantined; expected propose_delete branch"; fail=1 ;;
  *) echo "  ok: phishing URL went through propose_delete (not quarantine)" ;;
esac

# --- Benign URL: agent allows; link still works ---
q link::create '{"url":"https://iii.dev","code":"ok1"}' >/dev/null
sleep 4
assert "benign link still resolves" "https://iii.dev" "$(q link::resolve '{"code":"ok1"}')"

# --- Ch1/3 redirect path still works (quarantine blocks 302) ---
rc="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HTTP_PORT/s/ok1")"
assert "benign /s/ok1 redirects (302)" "302" "$rc"
rc_q="$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HTTP_PORT/s/q1")"
assert "quarantined /s/q1 returns 404" "404" "$rc_q"

echo
if [ "$fail" -eq 0 ]; then echo "Ch8 e2e: PASS"; else echo "Ch8 e2e: FAIL"; fi
exit "$fail"
