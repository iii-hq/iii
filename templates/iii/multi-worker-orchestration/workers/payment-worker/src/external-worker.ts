// Example external worker — its endpoints are registered
// so that other workers connected to iii can trigger them.

import { registerWorker } from "iii-sdk";
const iii = registerWorker(process.env.III_URL ?? "ws://localhost:49134");

iii.registerFunction("payment-worker::record", async (payload) => {
  // A real worker would call an external API:
  // const result = await fetch("https://example.com/v1/payments/record", {
  //   method: "POST",
  //   body: JSON.stringify(payload),
  // });
  return {
    status: 200,
    body: { message: "Payment recorded" },
    source: "payment-worker",
  };
});

console.log("Payment worker started - listening for calls");
