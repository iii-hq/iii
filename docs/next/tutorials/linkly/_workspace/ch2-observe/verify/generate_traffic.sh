#!/usr/bin/env bash
# Seed one link and generate redirect traffic against the running engine.
# Run from the linkly project dir with `iii` already running in another terminal.
set -euo pipefail
HTTP="http://127.0.0.1:3111"

expect_code() { # expect_code <want> <curl args...>
  local want="$1"; shift
  local got; got="$(curl -s -o /dev/null -w '%{http_code}' "$@")"
  [ "$got" = "$want" ] || { echo "expected $want, got $got for: $*" >&2; exit 1; }
}

seed() {
  curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
    -d '{"url":"https://iii.dev","code":"iii"}' >/dev/null
}

seed
# Routes only bind once link has registered while iii-http is active.
# On a cold boot that can race; restart the worker once if the route 404s.
if [ "$(curl -s -o /dev/null -w '%{http_code}' "$HTTP/s/iii")" = "404" ]; then
  echo "routes not bound yet; restarting link..."
  iii worker restart link >/dev/null 2>&1 || true
  sleep 2
  seed
fi

for _ in $(seq 1 10); do expect_code 302 "$HTTP/s/iii"; done
expect_code 404 "$HTTP/s/missing"
echo "seeded: 1 create, 10 redirects (302), 1 miss (404)"
