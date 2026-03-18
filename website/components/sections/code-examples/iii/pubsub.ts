import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "events-iii",
  },
);

iii.registerFunction(
  { id: "orders::publish-created" },
  async (request: any) => {
    const logger = new Logger();
    const payload = {
      eventId: request.body.eventId ?? `evt-${Date.now()}`,
      orderId: request.body.orderId,
    };
    // ...validate and enrich...
    iii.trigger({
      function_id: "publish",
      payload: {
        topic: "order.created",
        data: payload,
      },
      action: TriggerAction.Void(),
    });
    logger.info("events.publish_order_created.published", {
      eventId: payload.eventId,
      orderId: payload.orderId,
    });
    return {
      accepted: true,
      eventId: payload.eventId,
    };
  },
);

iii.registerFunction({ id: "orders::project-created" }, async (event: any) => {
  const logger = new Logger();
  // ...idempotency check...
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "processed-events",
      key: event.eventId,
      value: {
        _key: event.eventId,
        eventId: event.eventId,
        status: "applied",
      },
    },
  });
  logger.info("events.consume_order_created.applied", {
    eventId: event.eventId,
  });
  return { applied: true };
});

iii.registerFunction({ id: "orders::processed-snapshot" }, async () => {
  const items = await iii.trigger({
    function_id: "state::list",
    payload: { scope: "processed-events" },
  });
  return { processedCount: items.length };
});

iii.registerTrigger({
  type: "subscribe",
  function_id: "orders::project-created",
  config: { topic: "order.created" },
});

iii.registerTrigger({
  type: "http",
  function_id: "orders::publish-created",
  config: {
    api_path: "/events/order-created",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "orders::processed-snapshot",
  config: {
    api_path: "/events/processed",
    http_method: "GET",
  },
});
