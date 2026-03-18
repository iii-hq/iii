import { registerWorker, Logger } from "iii-sdk";

const iii = registerWorker(process.env.III_ENGINE_URL || "ws://localhost:49134", {
  workerName: "logging-iii",
});

iii.registerFunction({ id: "checkout::calculate-totals" }, async (data: any) => {
  const logger = new Logger();
  const subtotal = Number(data.subtotal ?? 0);
  const tax = subtotal * 0.1;
  const total = subtotal + tax;
  logger.info("checkout.calculate_totals.done", {
    requestId: data.requestId,
    subtotal,
    tax,
    total,
  });
  return { subtotal, tax, total };
});

iii.registerFunction({ id: "checkout::charge-payment" }, async (data: any) => {
  const logger = new Logger();
  const approved = data.cardToken === "tok_valid";
  if (!approved) {
    logger.warn("checkout.charge_payment.declined", { requestId: data.requestId });
    const error = new Error("Payment declined") as Error & { status: number };
    error.status = 402;
    throw error;
  }
  logger.info("checkout.charge_payment.approved", { requestId: data.requestId, total: data.total });
  return { approved: true };
});

iii.registerFunction({ id: "checkout::execute" }, async (request: any) => {
  const logger = new Logger();
  const requestId = request.headers["x-request-id"] || `req-${Date.now()}`;
  logger.info("checkout.request.received", { requestId, cartId: request.body.cartId });
  const totals = await iii.trigger({
    function_id: "checkout::calculate-totals",
    payload: { requestId, subtotal: request.body.subtotal },
  });
  await iii.trigger({
    function_id: "checkout::charge-payment",
    payload: { requestId, total: totals.total, cardToken: request.body.cardToken },
  });
  logger.info("checkout.completed", { requestId, total: totals.total });
  return { requestId, total: totals.total, status: "paid" };
});

iii.registerTrigger({
  type: "http",
  function_id: "checkout::execute",
  config: { api_path: "/checkout", http_method: "POST" },
});
