import { registerWorker, Logger } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "remote-iii",
  },
);

iii.registerFunction(
  { id: "billing::create-invoice" },
  {
    url: `${process.env.BILLING_API_URL}/invoices`,
    method: "POST",
    timeout_ms: 5000,
    auth: {
      type: "bearer",
      token_key: "BILLING_API_TOKEN",
    },
  },
);

async function invokeWithRetry(payload: any) {
  const logger = new Logger();
  for (let attempt = 1; attempt <= 2; attempt++) {
    try {
      return await iii.trigger({
        function_id: "billing::create-invoice",
        payload,
      });
    } catch (error) {
      if (attempt < 2) {
        logger.warn("remote.create_invoice.retry", {
          attempt,
          retriesLeft: 2 - attempt,
        });
        continue;
      }
      throw error;
    }
  }
  throw new Error("invoice request failed");
}

iii.registerFunction({ id: "remote::create-invoice" }, async (request: any) => {
  const logger = new Logger();
  const invoice = await invokeWithRetry({
    customerId: request.body.customerId,
    amount: request.body.amount,
  });
  // ...response mapping...
  logger.info("remote.create_invoice.completed", {
    invoiceId: invoice.id,
  });
  return invoice;
});

iii.registerTrigger({
  type: "http",
  function_id: "remote::create-invoice",
  config: {
    api_path: "/remote/invoices",
    http_method: "POST",
  },
});
