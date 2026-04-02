import { registerWorker, Logger } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "feature-flags-iii",
  },
);

iii.registerFunction("flags::set-override", async (request: any) => {
  const logger = new Logger();
  const flagKey = request.params.flagKey;
  const record = await iii.trigger({
    function_id: "flags-service::normalize-override",
    payload: {
      flagKey,
      value: request.body.value,
      actorId: request.body.actorId ?? "system",
    },
  });
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "flag-overrides",
      key: flagKey,
      value: record,
    },
  });
  logger.info("flags.override_set", record);
  return record;
});

iii.registerFunction("flags::evaluate", async (request: any) => {
  const logger = new Logger();
  const flagKey = request.params.flagKey;
  const userId = String(request.query.userId || "anonymous");
  const override = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "flag-overrides",
      key: flagKey,
    },
  });
  const evaluation = await iii.trigger({
    function_id: "flags-service::evaluate",
    payload: {
      flagKey,
      userId,
      override: override?.value,
      attributes: request.query,
    },
  });
  logger.info("flags.evaluate.completed", {
    flagKey,
    userId,
    value: evaluation.value,
    source: evaluation.source,
  });
  return evaluation;
});

iii.registerTrigger({
  type: "http",
  function_id: "flags::set-override",
  config: {
    api_path: "/flags/:flagKey/overrides",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "flags::evaluate",
  config: {
    api_path: "/flags/:flagKey/evaluate",
    http_method: "GET",
  },
});
