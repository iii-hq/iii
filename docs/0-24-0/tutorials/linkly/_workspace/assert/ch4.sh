set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

for n in 1 2 3 4 5; do
  curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
    -d "{\"url\":\"https://iii.dev/$n\",\"code\":\"analyticslink$n\"}" >/dev/null
done
for _ in 1 2 3; do curl -s -o /dev/null "$HTTP/s/analyticslink1"; done
sleep 5

check "analytics counted the links" '"count": 5' \
  "$(t database::query db=analytics sql="SELECT day, count FROM daily_link_counts")"
check "clicks drained from the queue" '"clicks": 3' \
  "$(t database::query db=primary sql="SELECT COUNT(*) AS clicks FROM clicks WHERE code = 'analyticslink1'")"

check "PUT /links/:code" 'https://iii.dev/updated' \
  "$(curl -s -X PUT "$HTTP/links/analyticslink1" -H 'Content-Type: application/json' \
    -d '{"url":"https://iii.dev/updated"}')"
sleep 3
check "cache refreshed by durable subscriber" 'https://iii.dev/updated' \
  "$(t link::resolve code=analyticslink1)"

finish
