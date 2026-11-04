set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# The chapter runs without iii-stream and pubsub.
check "compose file has no iii-stream or pubsub" 'absent' \
  "$(grep -qE 'iii-stream|pubsub' "$PROJECT/worker-compose.yaml" && echo present || echo absent)"

# A listener observes click-streamer::on-click, the way watch.ts does in the chapter.
watch_log="$(mktemp)"
(cd "$PROJECT/click-streamer" && exec node --import tsx watch.ts) >"$watch_log" 2>&1 &
watcher=$!
sleep 8
check "watcher read the starting total" 'recorded so far' "$(cat "$watch_log")"

# The feed is private: only click-streamer may call it.
check "the feed refuses calls from other workers" 'FORBIDDEN' \
  "$(t click-streamer::on-click --json '{"id":1000000,"code":"forged","clicked_at":"2026-01-01T00:00:00Z"}')"

# One click straight through link::record_click: click-streamer observes it,
# calls the feed, and the feed's observer (the watcher) prints the click.
check "link::record_click returns the click" '"code": "direct"' \
  "$(t link::record_click code=direct clicked_at=2026-01-01T00:00:00Z)"

# The whole click path: redirect enqueues, the queue drains into the clicks
# table, click-streamer reacts to each recorded click, the watcher prints it.
curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"stream-me"}' >/dev/null
for _ in 1 2 3; do curl -s -o /dev/null "$HTTP/s/stream-me"; done
sleep 10

check "clicks recorded" '"clicks": 3' \
  "$(t database::query db=primary sql="SELECT COUNT(*) AS clicks FROM clicks WHERE code = 'stream-me'")"
check "watcher printed the direct click" 'direct at 2026-01-01T00:00:00Z' "$(cat "$watch_log")"
check "watcher printed no forged click" '0' "$(grep -c 'forged' "$watch_log")"
check "watcher printed every click" '3' "$(grep -c ': stream-me at ' "$watch_log")"
# Three redirects and the direct click.
check "click summary counts the clicks" '"total": 4' "$(t link::click_summary)"

kill "$watcher" 2>/dev/null
wait "$watcher" 2>/dev/null
echo "  watcher output:"
sed 's/^/    /' "$watch_log"
rm -f "$watch_log"

# Filters belong to iii: an observer filtered on input.code "" hears only
# clicks whose code is "", and an observer without a filter hears every click.
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
await Promise.all([
  worker
    .observe(
      {
        function_id: "click-streamer::on-click",
        filters: { "input.code": { $eq: "" } },
        deliver: "output",
      },
      "filter-check::empty",
    )
    .ready(),
  worker
    .observe({ function_id: "click-streamer::on-click", deliver: "output" }, "filter-check::none")
    .ready(),
]);
const click = await worker.call({
  function_id: "link::record_click",
  payload: { code: "other", clicked_at: "2026-01-02T00:00:00Z" },
});
await new Promise((r) => setTimeout(r, 2000));
console.log(JSON.stringify({ code: (click as { code: string }).code, received }));
process.exit(0);
EOF
filter_out="$(cd "$PROJECT/click-streamer" && timeout 60 node --import tsx filter-check.ts 2>&1)"
echo "       filter check: $filter_out"
check "a filter on input.code selects clicks, no filter hears every click" \
  '{"code":"other","received":{"empty":[],"none":["other"]}}' "$filter_out"
rm -f "$PROJECT/click-streamer/filter-check.ts"

finish
