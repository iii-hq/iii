// Account Events Worker
// This worker manages account upgrades and publishes events to the iii event bus.
// Follow the steps below to progressively build out the full system.

import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? "ws://localhost:49134",
);
const logger = new Logger();

const LEGACY_WORKER_URL =
  process.env.LEGACY_WORKER_URL ?? "http://localhost:8080";

// Mock account database
const accounts: Record<
  string,
  { name: string; plan: string; arr: number }
> = {
  "acme-corp": { name: "Acme Corp", plan: "enterprise", arr: 120_000 },
  "small-biz": { name: "Small Biz Co", plan: "starter", arr: 5_000 },
  "mega-inc": { name: "Mega Inc", plan: "enterprise", arr: 250_000 },
  "tiny-startup": { name: "Tiny Startup", plan: "free", arr: 0 },
};

// ═══════════════════════════════════════════════════════════════════
// STEP 1: Account upgrade events (active by default)
// ═══════════════════════════════════════════════════════════════════

// This function is called cross-worker by the ai-agent worker.
iii.registerFunction(
  { id: "accounts::get-details" },
  async (payload) => {
    const companyId = payload.companyId ?? payload.data?.companyId;
    const account = accounts[companyId];
    if (!account) return { error: "Account not found", companyId };
    return { companyId, ...account };
  },
);

// Simulate an account upgrade — publishes an event to the bus.
const onUpgrade = iii.registerFunction(
  { id: "accounts::on-upgrade" },
  async (payload) => {
    const companyId =
      payload.body?.companyId ?? payload.companyId ?? "acme-corp";
    logger.info("Account upgrade received", { companyId });

    // Publish the upgrade event so any subscriber can react
    iii.trigger({
      function_id: "publish",
      payload: { topic: "account.upgraded", data: { companyId } },
      action: TriggerAction.Void(),
    });

    return {
      status: 200,
      body: { published: true, companyId, topic: "account.upgraded" },
    };
  },
);

// HTTP endpoint so you can trigger an upgrade with curl
iii.registerTrigger({
  type: "http",
  function_id: onUpgrade.id,
  config: { api_path: "/account/upgrade", http_method: "POST" },
});

console.log(
  "✓ Step 1 complete: account-events worker connected. accounts::on-upgrade is live.",
);
console.log(
  '  → Try: curl -X POST http://localhost:3111/account/upgrade -H "Content-Type: application/json" -d \'{"companyId":"acme-corp"}\'',
);
console.log(
  "  → Next: Uncomment Step 2 below (the sales notification subscriber), then save to restart.",
);
console.log("");

// ═══════════════════════════════════════════════════════════════════
// STEP 2: Sales team notifications
// Uncomment the block below to subscribe a sales notification
// function to the account.upgraded topic. Every time an account
// upgrades, the sales team will be notified automatically.
// ═══════════════════════════════════════════════════════════════════

// --- UNCOMMENT STEP 2 START ---
// iii.registerFunction(
//   { id: "sales::notify-team" },
//   async (event) => {
//     const companyId = event.companyId ?? event.data?.companyId;
//     logger.info("Sales team notified", { companyId });
//     console.log(
//       `  [Sales] Notification: ${companyId} just upgraded their plan!`,
//     );
//     return { notified: true, companyId };
//   },
// );
//
// iii.registerTrigger({
//   type: "subscribe",
//   function_id: "sales::notify-team",
//   config: { topic: "account.upgraded" },
// });
//
// console.log(
//   "✓ Step 2 complete: Sales team will be notified on account upgrades.",
// );
// console.log(
//   "  → Next: Start the ai-agent worker (see README for instructions).",
// );
// console.log("");
// --- UNCOMMENT STEP 2 END ---

// ═══════════════════════════════════════════════════════════════════
// STEP 4: Legacy system integration
// Uncomment AFTER starting the legacy-worker (Java HTTP server).
// This registers an iii function that proxies to the legacy server.
// The legacy code is never modified — iii connects to it externally.
// ═══════════════════════════════════════════════════════════════════

// --- UNCOMMENT STEP 4 START ---
// iii.registerFunction(
//   { id: "legacy::get-status" },
//   async (payload) => {
//     const companyId =
//       payload.body?.companyId ?? payload.companyId ?? "unknown";
//     try {
//       const response = await fetch(
//         `${LEGACY_WORKER_URL}/api/status?company=${companyId}`,
//       );
//       const data = await response.json();
//       logger.info("Legacy system responded", { companyId });
//       return data;
//     } catch (error) {
//       logger.error("Legacy system unreachable", {
//         error: String(error),
//       });
//       return { error: "Legacy system unreachable", companyId };
//     }
//   },
// );
//
// iii.registerTrigger({
//   type: "http",
//   function_id: "legacy::get-status",
//   config: { api_path: "/legacy/status", http_method: "POST" },
// });
//
// console.log(
//   "✓ Step 4 complete: Legacy system connected without touching its code.",
// );
// console.log(
//   "  → Next: Uncomment Step 5 (the ARR filter) in workers/ai-agent/agent.py",
// );
// console.log("");
// --- UNCOMMENT STEP 4 END ---
