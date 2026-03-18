import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(process.env.III_ENGINE_URL || "ws://localhost:49134", {
  workerName: "feature-flags-iii",
});

iii.registerFunction({ id: "flags::set-override" }, async (request: any) => {
  const logger = new Logger();
  const flagKey = request.params.flagKey;
  const value = Boolean(request.body.value);
  const record = { _key: flagKey, flagKey, value, updatedAt: new Date().toISOString() };
  await iii.trigger({
    function_id: "state::set",
    payload: { scope: "flag-overrides", key: flagKey, value: record },
  });
  logger.info("flags.override_set", record);
  return record;
});

iii.registerFunction({ id: "flags::evaluate" }, async (request: any) => {
  const logger = new Logger();
  const flagKey = request.params.flagKey;
  const userId = String(request.query.userId || "anonymous");
  const override = await iii.trigger({
    function_id: "state::get",
    payload: { scope: "flag-overrides", key: flagKey },
  });
  if (override) {
    logger.info("flags.evaluate.override", { flagKey, userId, value: override.value });
    return { flagKey, userId, value: override.value, source: "override" };
  }
  const value = false;
  logger.info("flags.evaluate.default", { flagKey, userId, value });
  return { flagKey, userId, value, source: "default" };
});

iii.registerFunction({ id: "flags::on-updated" }, async (event: any) => {
  const logger = new Logger();
  iii.trigger({
    function_id: "stream::send",
    payload: {
      stream_name: "flags-updates",
      group_id: "global",
      id: `flag-${Date.now()}`,
      event_type: "flag.updated",
      data: event.new_value,
    },
    action: TriggerAction.Void(),
  });
  logger.info("flags.update_propagated", event.new_value);
  return { ok: true };
});

iii.registerTrigger({
  type: "state",
  function_id: "flags::on-updated",
  config: { scope: "flag-overrides" },
});

iii.registerTrigger({
  type: "http",
  function_id: "flags::set-override",
  config: { api_path: "/flags/:flagKey/overrides", http_method: "POST" },
});

iii.registerTrigger({
  type: "http",
  function_id: "flags::evaluate",
  config: { api_path: "/flags/:flagKey/evaluate", http_method: "GET" },
});
