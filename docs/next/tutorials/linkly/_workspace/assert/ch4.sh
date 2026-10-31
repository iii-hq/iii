set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# One field of the first row a database::query printed.
first_row() {
  python3 -c 'import json, sys; s = sys.stdin.read(); print(json.loads(s[s.index("{"):])["rows"][0][sys.argv[1]])' "$1"
}

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

check_duplicate analyticslink2
check_missing_url
check "a duplicate publishes no link.created" '"count": 5' \
  "$(sleep 3; t database::query db=analytics sql="SELECT day, count FROM daily_link_counts")"

# The counted day is the UTC day of the created_at link::create stored.
created_at="$(t database::query db=primary sql="SELECT created_at FROM links WHERE code = 'analyticslink3'" |
  first_row created_at)"
check "link_events day is the day of links.created_at" "\"day\": \"${created_at:0:10}\"" \
  "$(t database::query db=analytics sql="SELECT code, day FROM link_events WHERE code = 'analyticslink3'")"

# At least once: the same link.created delivered again, through the topic and
# straight to the handler, does not count twice.
event="{\"code\":\"analyticslink3\",\"url\":\"https://iii.dev/3\",\"created_at\":\"$created_at\"}"
echo "       republishing $event"
t iii::durable::publish --json "{\"topic\":\"link.created\",\"data\":$event}" >/dev/null
t iii::durable::publish --json "{\"topic\":\"link.created\",\"data\":$event}" >/dev/null
check "handler called again for the same link" '"ok": true' "$(t analytics::on_link_created --json "$event")"
sleep 3
check "a redelivered link.created keeps the count" '"count": 5' \
  "$(t database::query db=analytics sql="SELECT day, count FROM daily_link_counts")"
check "one link_events row for the redelivered link" '"rows": 1' \
  "$(t database::query db=analytics sql="SELECT COUNT(*) AS rows FROM link_events WHERE code = 'analyticslink3'")"

# The day comes from the event, not from the delivery time.
old='{"code":"backdated","url":"https://iii.dev/old","created_at":"2020-01-02T23:59:59.000Z"}'
t iii::durable::publish --json "{\"topic\":\"link.created\",\"data\":$old}" >/dev/null
t iii::durable::publish --json "{\"topic\":\"link.created\",\"data\":$old}" >/dev/null
sleep 3
check "a backdated event counts on its own day, once" '"count": 1' \
  "$(t database::query db=analytics sql="SELECT day, count FROM daily_link_counts WHERE day = '2020-01-02'")"

# A second subscriber shows the link.created payload itself.
cat >"$PROJECT/link/peek-created.mjs" <<'EOF'
import { registerWorker } from "iii-sdk";
const worker = registerWorker(process.env.III_URL, {
  workerName: "peek-created",
  namespace: process.env.III_NAMESPACE || "default",
});
worker.registerFunction("peek-created::print", async (data) => {
  console.log(JSON.stringify(data));
  return null;
});
worker.observe(
  { function_id: "queue::on-message", args: { topic: "link.created" } },
  "peek-created::print",
);
EOF
peek_log="$(mktemp)"
(cd "$PROJECT/link" && exec node peek-created.mjs) >"$peek_log" 2>&1 &
peeker=$!
sleep 5
curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev/peek","code":"peeklink"}' >/dev/null
sleep 5
kill "$peeker" 2>/dev/null
wait "$peeker" 2>/dev/null
peek_created_at="$(t database::query db=primary sql="SELECT created_at FROM links WHERE code = 'peeklink'" |
  first_row created_at)"
check "link.created carries the stored created_at" "\"created_at\":\"$peek_created_at\"" \
  "$(grep '"code":"peeklink"' "$peek_log")"
echo "       link.created payload: $(grep peeklink "$peek_log")"
rm -f "$peek_log" "$PROJECT/link/peek-created.mjs"
echo "       daily_link_counts: $(t database::query db=analytics sql="SELECT day, count FROM daily_link_counts ORDER BY day" | tr -s ' \n' ' ')"

check "PUT /links/:code" 'https://iii.dev/updated' \
  "$(curl -s -X PUT "$HTTP/links/analyticslink1" -H 'Content-Type: application/json' \
    -d '{"url":"https://iii.dev/updated"}')"
sleep 3
check "cache refreshed by the link.updated subscriber" 'https://iii.dev/updated' \
  "$(t link::resolve code=analyticslink1)"

finish
