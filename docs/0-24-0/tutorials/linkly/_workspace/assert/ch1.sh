set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

check "POST /links" '"code":"iii"' \
  "$(curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
    -d '{"url":"https://iii.dev","code":"iii"}')"

check "link::resolve" 'https://iii.dev' "$(t link::resolve code=iii)"
check "link::resolve unknown" 'null' "$(t link::resolve code=nope)"
check "GET /s/:code" 'location: https://iii.dev' \
  "$(curl -s -i "$HTTP/s/iii" | tr 'A-Z' 'a-z')"
check "GET /s/:code 302" '302' "$(curl -s -o /dev/null -w '%{http_code}' "$HTTP/s/iii")"
check "GET unknown code" '404' "$(curl -s -o /dev/null -w '%{http_code}' "$HTTP/s/missing")"

check "POST /links duplicate code 409" '409' \
  "$(curl -s -o /dev/null -w '%{http_code}' -X POST "$HTTP/links" \
    -H 'Content-Type: application/json' -d '{"url":"https://example.com","code":"iii"}')"
check "duplicate did not overwrite" 'https://iii.dev' "$(t link::resolve code=iii)"

finish
