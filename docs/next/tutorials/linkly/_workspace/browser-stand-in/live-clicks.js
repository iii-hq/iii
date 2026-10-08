// Stands in for the Chapter 7 live click counter. It connects through the
// RBAC-gated listener in its own per-session namespace, binds ui::on_click to
// click-streamer::click, reads the starting total with link::click_summary,
// then follows a link and counts the clicks that arrive. Same logic as the
// useEffect in the chapter's App.tsx.
import { registerWorker } from "iii-browser-sdk";
import { randomUUID } from "node:crypto";

const TOKEN = process.env.LINKLY_BROWSER_TOKEN ?? "dev-token";
const BROWSER_URL = process.env.BROWSER_URL ?? "ws://localhost:3110";
const HTTP = process.env.HTTP ?? "http://127.0.0.1:3111";
const CODE = process.env.LINKLY_CODE ?? "watchme";
const SESSION = randomUUID();

const worker = registerWorker(
  `${BROWSER_URL}?token=${encodeURIComponent(TOKEN)}&session=${SESSION}`,
  { namespace: `browser-${SESSION}` },
);

let lastId = 0;
let count = 0;
let received = 0;
let ready = false;
const early = [];

function apply(click) {
  if (click.id <= lastId) return;
  lastId = click.id;
  count += 1;
}

async function sync() {
  ready = false;
  const summary = await worker.trigger({
    function_id: "link::click_summary",
    namespace: "default",
    payload: {},
  });
  lastId = summary.last_id;
  count = summary.total;
  ready = true;
  early.splice(0).forEach(apply);
}

worker.registerFunction("ui::on_click", async (click) => {
  received += 1;
  if (ready) apply(click);
  else early.push(click);
  return null;
});
worker.registerTrigger({
  type: "click-streamer::click",
  function_id: "ui::on_click",
  config: {},
});

await new Promise((resolve) => setTimeout(resolve, 3000));
await sync();
const initial = count;

for (let i = 0; i < 3; i++) {
  await fetch(`${HTTP}/s/${CODE}`, { redirect: "manual" });
}
await new Promise((resolve) => setTimeout(resolve, 8000));

console.log(JSON.stringify({ initial, received, count }));
await worker.shutdown();
