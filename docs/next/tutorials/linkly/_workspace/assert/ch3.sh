set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"iii"}' >/dev/null
for _ in 1 2 3; do curl -s -o /dev/null "$HTTP/s/iii"; done
sleep 2

check "clicks recorded" '"clicks": 3' \
  "$(t database::query db=primary sql="SELECT COUNT(*) AS clicks FROM clicks WHERE code = 'iii'")"
check "link row persisted" 'https://iii.dev' \
  "$(t database::query db=primary sql="SELECT url FROM links WHERE code = 'iii'")"

finish
