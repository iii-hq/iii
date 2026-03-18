import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "ai-agents-iii",
  },
);

iii.registerFunction({ id: "agents::plan" }, async (data: any) => {
  return iii.trigger({
    function_id: "planner-service::plan",
    payload: data,
  });
});

iii.registerFunction({ id: "agents::retrieve" }, async (data: any) => {
  return iii.trigger({
    function_id: "knowledge-service::retrieve-context",
    payload: data,
  });
});

iii.registerFunction({ id: "agents::respond" }, async (data: any) => {
  const result = await iii.trigger({
    function_id: "llm-service::respond",
    payload: data,
  });
  for (const token of result.previewTokens ?? []) {
    iii.trigger({
      function_id: "stream::send",
      payload: {
        stream_name: "agent-stream",
        group_id: data.sessionId,
        id: `tok-${Date.now()}-${Math.random()}`,
        event_type: "token",
        data: { token },
      },
      action: TriggerAction.Void(),
    });
  }
  return { answer: result.answer };
});

iii.registerFunction({ id: "agents::persist" }, async (data: any) => {
  return iii.trigger({
    function_id: "memory-service::persist-turn",
    payload: data,
  });
});

iii.registerFunction({ id: "agents::chat" }, async (request: any) => {
  const logger = new Logger();
  const sessionId = request.body.sessionId ?? `session-${Date.now()}`;
  const input = String(request.body.input ?? "");
  const plan = await iii.trigger({
    function_id: "agents::plan",
    payload: { sessionId, input },
  });
  const retrieval = plan.needsDocs
    ? await iii.trigger({
        function_id: "agents::retrieve",
        payload: { sessionId, input },
      })
    : { docs: [] };
  const response = await iii.trigger({
    function_id: "agents::respond",
    payload: {
      sessionId,
      input,
      docs: retrieval.docs,
    },
  });
  await iii.trigger({
    function_id: "agents::persist",
    payload: {
      sessionId,
      input,
      answer: response.answer,
    },
  });
  iii.trigger({
    function_id: "analytics-service::track-chat",
    payload: {
      sessionId,
      inputLength: input.length,
      answerLength: response.answer.length,
    },
    action: TriggerAction.Void(),
  });
  logger.info("agents.chat.completed", {
    sessionId,
  });
  return { sessionId, answer: response.answer };
});

iii.registerTrigger({
  type: "http",
  function_id: "agents::chat",
  config: {
    api_path: "/agents/chat",
    http_method: "POST",
  },
});
