set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

check "browser listener open" 'open' \
  "$(nc -z 127.0.0.1 3110 >/dev/null 2>&1 && echo open || echo closed)"

curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"deleteme"}' >/dev/null

# A Node client stands in for the browser tab: it connects through the
# RBAC-gated listener and answers the server's confirmation call.
check "browser worker confirms the delete" '"deleted":true' \
  "$(cd "$PROJECT/browser-stand-in" && node confirm.js 2>&1 | tail -1)"
sleep 2
check "link deleted" 'null' "$(t link::resolve code=deleteme)"

finish
