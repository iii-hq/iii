// Stands in for the Chapter 7 browser tab. It connects through the RBAC-gated
// listener, registers user::confirm_destructive_op, and calls
// link::request_delete so the engine calls back into this process.
import { registerWorker } from "iii-browser-sdk";

const TOKEN = process.env.LINKLY_BROWSER_TOKEN ?? "dev-token";
const CODE = process.env.LINKLY_CODE ?? "deleteme";

const worker = registerWorker(`ws://localhost:3110?token=${encodeURIComponent(TOKEN)}`);

worker.registerFunction("user::confirm_destructive_op", async () => ({ confirmed: true }));

await new Promise((resolve) => setTimeout(resolve, 3000));

const result = await worker.trigger({
  function_id: "link::request_delete",
  payload: { code: CODE },
});
console.log(JSON.stringify(result));

await worker.shutdown();
