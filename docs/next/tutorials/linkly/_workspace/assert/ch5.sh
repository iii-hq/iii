set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# The broadcast path on its own: click-streamer writes to the clicks stream.
t click-streamer::broadcast code=direct clicked_at=2026-01-01T00:00:00Z >/dev/null
sleep 2
check "broadcast reaches the stream" 'direct' \
  "$(t stream::list stream_name=clicks group_id=all)"

# The whole click path: redirect enqueues, the queue drains, the stream receives.
curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"stream-me"}' >/dev/null
for _ in 1 2 3; do curl -s -o /dev/null "$HTTP/s/stream-me"; done
sleep 10

check "clicks recorded" '"clicks": 3' \
  "$(t database::query db=primary sql="SELECT COUNT(*) AS clicks FROM clicks WHERE code = 'stream-me'")"
check "clicks reached the stream" 'stream-me' \
  "$(t stream::list stream_name=clicks group_id=all)"

finish
