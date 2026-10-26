set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# The chapter runs without iii-stream and pubsub.
check "compose file has no iii-stream or pubsub" 'absent' \
  "$(grep -qE 'iii-stream|pubsub' "$PROJECT/worker-compose.yaml" && echo present || echo absent)"

# A listener binds to click-streamer::click, the way watch.ts does in the chapter.
watch_log="$(mktemp)"
(cd "$PROJECT/click-streamer" && exec node --import tsx watch.ts) >"$watch_log" 2>&1 &
watcher=$!
sleep 8
check "watcher read the starting total" 'recorded so far' "$(cat "$watch_log")"

# The broadcast path on its own: click-streamer delivers to the bound watcher.
check "broadcast reaches the bound listener" '"delivered": 1' \
  "$(t click-streamer::broadcast --json '{"id":1000000,"code":"direct","clicked_at":"2026-01-01T00:00:00Z"}')"

# The whole click path: redirect enqueues, the queue drains into the clicks
# table, link announces each committed row, click-streamer delivers it.
curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"stream-me"}' >/dev/null
for _ in 1 2 3; do curl -s -o /dev/null "$HTTP/s/stream-me"; done
sleep 10

check "clicks recorded" '"clicks": 3' \
  "$(t database::query db=primary sql="SELECT COUNT(*) AS clicks FROM clicks WHERE code = 'stream-me'")"
check "watcher printed the direct broadcast" 'direct at 2026-01-01T00:00:00Z' "$(cat "$watch_log")"
check "watcher printed every click" '3' "$(grep -c ': stream-me at ' "$watch_log")"
check "click summary counts the clicks" '"total": 3' "$(t link::click_summary)"

kill "$watcher" 2>/dev/null
wait "$watcher" 2>/dev/null
echo "  watcher output:"
sed 's/^/    /' "$watch_log"
rm -f "$watch_log"

# The code filter: a binding with code "" hears only clicks whose code is "",
# and a binding without a code hears every click.
cat >"$PROJECT/click-streamer/filter-check.ts" <<'EOF'
import { registerWorker } from "iii-sdk";

const worker = registerWorker(process.env.III_URL!, {
  workerName: "filter-check",
  namespace: process.env.III_NAMESPACE || "default",
});
const received: Record<string, string[]> = { empty: [], none: [] };
for (const name of ["empty", "none"]) {
  worker.registerFunction(`filter-check::${name}`, async (click: { code: string }) => {
    received[name].push(click.code);
    return null;
  });
}
worker.registerTrigger({ type: "click-streamer::click", function_id: "filter-check::empty", config: { code: "" } });
worker.registerTrigger({ type: "click-streamer::click", function_id: "filter-check::none", config: {} });
await new Promise((r) => setTimeout(r, 3000));
const broadcast = await worker.trigger({
  function_id: "click-streamer::broadcast",
  payload: { id: 2000000, code: "other", clicked_at: "2026-01-02T00:00:00Z" },
});
await new Promise((r) => setTimeout(r, 2000));
console.log(JSON.stringify({ broadcast, received }));
process.exit(0);
EOF
filter_out="$(cd "$PROJECT/click-streamer" && timeout 60 node --import tsx filter-check.ts 2>&1)"
echo "       filter check: $filter_out"
check "code \"\" filters, no code hears every click" \
  '{"broadcast":{"delivered":1},"received":{"empty":[],"none":["other"]}}' "$filter_out"
rm -f "$PROJECT/click-streamer/filter-check.ts"

finish
