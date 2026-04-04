import { registerWorker, Logger, TriggerAction } from 'iii-sdk';

const iii = registerWorker(
  process.env.III_ENGINE_URL || 'ws://localhost:49134',
  {
    workerName: 'events-iii',
  },
);

iii.registerFunction(
  'orders::publish-created',
  async (request: any) => {
    const logger = new Logger();
    const payload = {
      eventId: request.body.eventId ?? `evt-${Date.now()}`,
      orderId: request.body.orderId,
    };
    iii.trigger({
      function_id: 'publish',
      payload: {
        topic: 'order.created',
        data: payload,
      },
      action: TriggerAction.Void(),
    });
    logger.info('events.publish_order_created.published', {
      eventId: payload.eventId,
      orderId: payload.orderId,
    });
    return {
      accepted: true,
      eventId: payload.eventId,
    };
  },
);

iii.registerFunction('orders::consume-created', async (event: any) => {
  const logger = new Logger();
  logger.info('events.consume_order_created.ack', {
    eventId: event.eventId,
    orderId: event.orderId,
  });
  return { ack: true };
});

iii.registerTrigger({
  type: 'subscribe',
  function_id: 'orders::consume-created',
  config: { topic: 'order.created' },
});

iii.registerTrigger({
  type: 'http',
  function_id: 'orders::publish-created',
  config: {
    api_path: '/events/order-created',
    http_method: 'POST',
  },
});
