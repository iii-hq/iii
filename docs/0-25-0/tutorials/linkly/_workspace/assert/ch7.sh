set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

check "browser listener open" 'open' \
  "$(nc -z 127.0.0.1 "${BROWSER_PORT:-3110}" >/dev/null 2>&1 && echo open || echo closed)"

curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"deleteme"}' >/dev/null

# A Node client stands in for the browser tab: it connects through the
# RBAC-gated listener and answers the server's confirmation call.
check "browser worker confirms the delete" '"deleted":true' \
  "$(cd "$PROJECT/browser-stand-in" && node confirm.js 2>&1 | tail -1)"
sleep 2
check "link deleted" 'null' "$(t link::resolve code=deleteme)"

# The live click counter: the tab binds click-streamer::click through the
# proxy, reads the starting total, then counts the clicks that arrive.
curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"watchme"}' >/dev/null
curl -s -o /dev/null "$HTTP/s/watchme"
sleep 5
live="$(cd "$PROJECT/browser-stand-in" && node live-clicks.js 2>&1 | tail -1)"
echo "  live-clicks: $live"
check "browser read the starting total" '"initial":1' "$live"
check "browser received the live clicks" '"received":3' "$live"
check "browser counter is current" '"count":4' "$live"

finish
