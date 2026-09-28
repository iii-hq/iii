// Stands in for the Chapter 7 browser tab. It connects through the RBAC-gated
// listener in its own per-session namespace, registers
// user::confirm_destructive_op there, and calls link::request_delete (naming
// its session) so the engine calls back into this process.
import { registerWorker } from "iii-browser-sdk";
import { randomUUID } from "node:crypto";

const TOKEN = process.env.LINKLY_BROWSER_TOKEN ?? "dev-token";
const CODE = process.env.LINKLY_CODE ?? "deleteme";
const SESSION = randomUUID();

const worker = registerWorker(
  `ws://localhost:3110?token=${encodeURIComponent(TOKEN)}&session=${SESSION}`,
  { namespace: `browser-${SESSION}` },
);

worker.registerFunction("user::confirm_destructive_op", async () => ({ confirmed: true }));

await new Promise((resolve) => setTimeout(resolve, 3000));

const result = await worker.trigger({
  function_id: "link::request_delete",
  namespace: "default",
  payload: { code: CODE, session: SESSION },
});
console.log(JSON.stringify(result));

await worker.shutdown();
