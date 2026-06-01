#!/usr/bin/env bash
# Ch7 end-to-end validation (NOT user-facing). Each link can carry a Node.js
# rule script; on every redirect link boots a node sandbox, pipes the
# request as JSON on stdin, and uses the rule's `chosen_url`. Assertions:
# geo-routing rule, A/B split rule, fallback when there is no rule, and host
# isolation (host paths are unreachable from inside the sandbox).
#
# Requires hardware virtualization on the host (macOS Apple Silicon, Linux KVM).
#
# Usage: ./e2e.sh   (run from this directory)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT="$HERE/linkly"
LOG="$(mktemp -t linkly-ch7-engine.XXXX.log)"
TRUSTED_PORT=49134
HTTP_PORT=3111
STREAM_PORT=3112
CONSOLE_PORT=3113
BROWSER_PORT=3110
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

cd "$PROJECT"
rm -rf data
iii > "$LOG" 2>&1 &
ENGINE_PID=$!
wait_for_port "$TRUSTED_PORT" 60 || { echo "trusted listener never opened"; cat "$LOG"; exit 1; }
wait_for_port "$HTTP_PORT" 60 || { echo "iii-http never opened"; cat "$LOG"; exit 1; }
iii worker restart link >/dev/null 2>&1 || true
sleep 3

# Helper: get the Location header from a single redirect (no follow).
redirect_to() { curl -s -o /dev/null -w '%{redirect_url}' "$@"; }

# --- Seed two links ---
iii trigger link::create url=https://default.example code=geo >/dev/null 2>&1
iii trigger link::create url=https://norule.example code=norule >/dev/null 2>&1

# --- Geo rule routes by header ---
GEO_RULE='let buf="";process.stdin.on("data",d=>buf+=d).on("end",()=>{const r=JSON.parse(buf);console.log(JSON.stringify({chosen_url:(r.headers["x-geo"]||r.headers["X-Geo"])==="EU"?"https://eu.example":"https://us.example"}))})'
iii trigger link::set_rule code=geo rule="$GEO_RULE" >/dev/null 2>&1
assert "geo rule sends default request to the US" "https://us.example" "$(redirect_to "http://127.0.0.1:$HTTP_PORT/s/geo")"
assert "geo rule sends X-Geo:EU to the EU"      "https://eu.example" "$(redirect_to -H 'X-Geo: EU' "http://127.0.0.1:$HTTP_PORT/s/geo")"

# --- A/B split by first byte of the User-Agent ---
AB_RULE='let buf="";process.stdin.on("data",d=>buf+=d).on("end",()=>{const r=JSON.parse(buf);const ua=r.headers["user-agent"]||"";const b=ua.charCodeAt(0)%2===0?"https://a.example":"https://b.example";console.log(JSON.stringify({chosen_url:b}))})'
iii trigger link::set_rule code=geo rule="$AB_RULE" >/dev/null 2>&1
assert "A/B rule routes UA=Bot (B=66 even) to a" "https://a.example" "$(redirect_to -H 'User-Agent: Bot'     "http://127.0.0.1:$HTTP_PORT/s/geo")"
assert "A/B rule routes UA=Chrome (C=67 odd) to b" "https://b.example" "$(redirect_to -H 'User-Agent: Chrome' "http://127.0.0.1:$HTTP_PORT/s/geo")"

# --- A link without a rule keeps its plain destination ---
assert "link without a rule uses its stored URL" "https://norule.example" "$(redirect_to "http://127.0.0.1:$HTTP_PORT/s/norule")"

# --- Host isolation: a malicious rule cannot read a path from the host ---
ISO_RULE='let buf="";process.stdin.on("data",d=>buf+=d).on("end",()=>{try{const fs=require("fs");fs.readFileSync(buf.includes("HOST_PATH=")?buf.split("HOST_PATH=")[1].trim():"/nope","utf8");console.log(JSON.stringify({chosen_url:"https://leak.example"}))}catch(e){console.log(JSON.stringify({chosen_url:"https://blocked.example?err="+encodeURIComponent(e.code||"unknown")}))}})'
# Plant a host-only path into ctx via a tiny rewriter rule so the test does not
# need to mutate the rule source: pass the host path through the URL field.
HOST_PATH="$PROJECT/config.yaml"
iii trigger link::set_rule code=geo rule="$ISO_RULE" >/dev/null 2>&1
# Update the link's stored url to carry the host path; the rule reads stdin
# (which includes the JSON ctx where url=...), tries fs.readFileSync on it.
iii trigger link::update code=geo url="HOST_PATH=$HOST_PATH" >/dev/null 2>&1
ISO_OUT="$(redirect_to "http://127.0.0.1:$HTTP_PORT/s/geo")"
# The HOST_PATH-routing rule is unusual; what matters is that the rule comes
# back with `blocked.example` (read failed) and NOT `leak.example`.
case "$ISO_OUT" in
  *leak.example*) echo "  FAIL: host isolation — sandbox read a host path: $ISO_OUT"; fail=1 ;;
  *blocked.example*) echo "  ok: sandbox cannot read host paths (got: $ISO_OUT)" ;;
  *) echo "  FAIL: unexpected isolation result: $ISO_OUT"; fail=1 ;;
esac

echo
if [ "$fail" -eq 0 ]; then echo "Ch7 e2e: PASS"; else echo "Ch7 e2e: FAIL"; fi
exit "$fail"
