import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "workflow-iii",
  },
);

async function trackStep(
  orderId: string,
  step: string,
  status: string,
  extra: Record<string, any> = {},
) {
  await iii.trigger({
    function_id: "state::update",
    payload: {
      scope: "workflow-orders",
      key: orderId,
      ops: [
        {
          type: "set",
          path: "currentStep",
          value: step,
        },
        {
          type: "set",
          path: "status",
          value: status,
        },
        {
          type: "set",
          path: "updatedAt",
          value: new Date().toISOString(),
        },
        ...Object.entries(extra).map(([path, value]) => ({
          type: "set",
          path,
          value,
        })),
      ],
    },
  });
}

iii.registerFunction({ id: "orders::start" }, async (request: any) => {
  const logger = new Logger();
  const orderId = request.body.orderId ?? `ord-${Date.now()}`;
  const orderDraft = await iii.trigger({
    function_id: "checkout-service::normalize-order",
    payload: {
      orderId,
      items: request.body.items ?? [],
      accountId: request.body.accountId ?? "anonymous",
    },
  });
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "workflow-orders",
      key: orderId,
      value: {
        _key: orderId,
        orderId,
        status: "queued",
        currentStep: "created",
        items: orderDraft.items,
        accountId: orderDraft.accountId,
        updatedAt: new Date().toISOString(),
      },
    },
  });
  await iii.trigger({
    function_id: "orders::validate",
    payload: {
      orderId,
    },
    action: TriggerAction.Enqueue({
      queue: "orders-workflow",
    }),
  });
  logger.info('workflow.start_order_fulfillment.queued', {
    orderId,
  });
  return { orderId };
});

iii.registerFunction({ id: "orders::validate" }, async (data: any) => {
  const logger = new Logger();
  await trackStep(data.orderId, "validate", "running");
  const snapshot = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "workflow-orders",
      key: data.orderId,
    },
  });
  if (!snapshot) {
    const error = new Error("Workflow order not found") as Error & {
      status: number;
    };
    error.status = 404;
    throw error;
  }
  const validation = await iii.trigger({
    function_id: "validation-service::validate-order",
    payload: {
      orderId: data.orderId,
      items: snapshot.items,
      accountId: snapshot.accountId,
    },
  });
  if (!validation.ok) {
    await trackStep(data.orderId, "validate", "failed", {
      failureReason: validation.reason,
    });
    const error = new Error(validation.reason ?? "Order validation failed") as Error & {
      status: number;
    };
    error.status = 422;
    throw error;
  }
  const payment = await iii.trigger({
    function_id: "billing-service::charge-order",
    payload: {
      orderId: data.orderId,
      accountId: snapshot.accountId,
      amount: validation.totalAmount,
    },
  });
  await trackStep(data.orderId, "validate", "complete", {
    paymentId: payment.paymentId,
  });
  await iii.trigger({
    function_id: "orders::ship",
    payload: { orderId: data.orderId },
    action: TriggerAction.Enqueue({
      queue: "orders-workflow",
    }),
  });
  logger.info("workflow.step.validate", {
    orderId: data.orderId,
    paymentId: payment.paymentId,
  });
  return { ok: true };
});

iii.registerFunction({ id: "orders::ship" }, async (data: any) => {
  const logger = new Logger();
  await trackStep(data.orderId, "ship", "running");
  const shipment = await iii.trigger({
    function_id: "shipping-service::create-shipment",
    payload: { orderId: data.orderId },
  });
  await iii.trigger({
    function_id: "state::update",
    payload: {
      scope: "workflow-orders",
      key: data.orderId,
      ops: [
        {
          type: "set",
          path: "trackingNumber",
          value: shipment.trackingNumber,
        },
        {
          type: "set",
          path: "status",
          value: "fulfilled",
        },
      ],
    },
  });
  await trackStep(data.orderId, "ship", "fulfilled");
  iii.trigger({
    function_id: "publish",
    payload: {
      topic: "order.fulfilled",
      data: {
        orderId: data.orderId,
        trackingNumber: shipment.trackingNumber,
      },
    },
    action: TriggerAction.Void(),
  });
  logger.info("workflow.step.ship", {
    orderId: data.orderId,
    trackingNumber: shipment.trackingNumber,
  });
  return { trackingNumber: shipment.trackingNumber };
});

iii.registerFunction({ id: "orders::snapshot" }, async (request: any) => {
  const logger = new Logger();
  const snapshot = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "workflow-orders",
      key: request.params.orderId,
    },
  });
  if (!snapshot) {
    const error = new Error('Workflow not found') as Error & {
      status: number;
    };
    error.status = 404;
    throw error;
  }
  logger.info('workflow.snapshot.loaded', {
    orderId: request.params.orderId,
    status: snapshot.status,
  });
  return snapshot;
});

iii.registerTrigger({
  type: "http",
  function_id: "orders::start",
  config: {
    api_path: "/workflows/order",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "orders::snapshot",
  config: {
    api_path: "/workflows/order/:orderId",
    http_method: "GET",
  },
});
