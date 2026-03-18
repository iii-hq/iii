import { registerWorker, Logger, TriggerAction } from 'iii-sdk';

const iii = registerWorker(
  process.env.III_ENGINE_URL || 'ws://localhost:49134',
  {
    workerName: 'workflow-iii',
  },
);

async function track(orderId: string, step: string, status: string) {
  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'workflow-orders',
      key: orderId,
      ops: [
        {
          type: 'set',
          path: 'currentStep',
          value: step,
        },
        {
          type: 'set',
          path: 'status',
          value: status,
        },
        {
          type: 'set',
          path: 'updatedAt',
          value: new Date().toISOString(),
        },
      ],
    },
  });
}

iii.registerFunction({ id: 'orders::start' }, async (request: any) => {
  const logger = new Logger();
  const orderId = request.body.orderId ?? `ord-${Date.now()}`;
  await iii.trigger({
    function_id: 'state::set',
    payload: {
      scope: 'workflow-orders',
      key: orderId,
      value: {
        _key: orderId,
        orderId,
        status: 'queued',
        currentStep: 'created',
        updatedAt: new Date().toISOString(),
      },
    },
  });
  await iii.trigger({
    function_id: 'orders::validate',
    payload: {
      orderId,
    },
    action: TriggerAction.Enqueue({
      queue: 'orders-workflow',
    }),
  });
  logger.info('workflow.start_order_fulfillment.queued', {
    orderId,
  });
  return { orderId };
});

iii.registerFunction({ id: 'orders::validate' }, async (data: any) => {
  const logger = new Logger();
  await track(data.orderId, 'validate', 'running');
  // ...validation and enrichment...
  await track(data.orderId, 'validate', 'complete');
  await iii.trigger({
    function_id: 'orders::ship',
    payload: { orderId: data.orderId },
    action: TriggerAction.Enqueue({
      queue: 'orders-workflow',
    }),
  });
  logger.info('workflow.step.validate', {
    orderId: data.orderId,
  });
  return { ok: true };
});

iii.registerFunction({ id: 'orders::ship' }, async (data: any) => {
  const logger = new Logger();
  await track(data.orderId, 'ship', 'running');
  const trackingNumber = `trk-${Date.now()}`; // ...call carrier...
  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'workflow-orders',
      key: data.orderId,
      ops: [
        {
          type: 'set',
          path: 'trackingNumber',
          value: trackingNumber,
        },
        {
          type: 'set',
          path: 'status',
          value: 'fulfilled',
        },
      ],
    },
  });
  await track(data.orderId, 'ship', 'fulfilled');
  iii.trigger({
    function_id: 'publish',
    payload: {
      topic: 'order.fulfilled',
      data: {
        orderId: data.orderId,
        trackingNumber,
      },
    },
    action: TriggerAction.Void(),
  });
  logger.info('workflow.step.ship', {
    orderId: data.orderId,
    trackingNumber,
  });
  return { trackingNumber };
});

iii.registerFunction({ id: 'orders::snapshot' }, async (request: any) => {
  const logger = new Logger();
  const snapshot = await iii.trigger({
    function_id: 'state::get',
    payload: {
      scope: 'workflow-orders',
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
  type: 'http',
  function_id: 'orders::start',
  config: {
    api_path: '/workflows/order',
    http_method: 'POST',
  },
});

iii.registerTrigger({
  type: 'http',
  function_id: 'orders::snapshot',
  config: {
    api_path: '/workflows/order/:orderId',
    http_method: 'GET',
  },
});
