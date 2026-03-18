import { registerWorker, Logger, TriggerAction } from "iii-sdk";

const iii = registerWorker(process.env.III_ENGINE_URL || "ws://localhost:49134", {
  workerName: "ai-agents-iii",
});

iii.registerFunction({ id: "agents::plan" }, async (data: any) => {
  const logger = new Logger();
  const needsDocs = /docs|policy|reference/i.test(data.input);
  logger.info("agents.plan.completed", { sessionId: data.sessionId, needsDocs });
  return { needsDocs };
});

iii.registerFunction({ id: "agents::retrieve" }, async (data: any) => {
  const logger = new Logger();
  const docs = await iii.trigger({
    function_id: "state::list",
    payload: { scope: "knowledge-base" },
  });
  const selected = docs.slice(0, 3).map((item: any) => item.content || item._key);
  logger.info("agents.retrieve.completed", { sessionId: data.sessionId, count: selected.length });
  return { docs: selected };
});

iii.registerFunction({ id: "agents::respond" }, async (data: any) => {
  const logger = new Logger();
  const answer = `I can help with "${data.input}". ${data.docs.join(" ")}`.trim();
  for (const token of answer.split(" ")) {
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
  logger.info("agents.respond.completed", { sessionId: data.sessionId, outputLength: answer.length });
  return { answer };
});

iii.registerFunction({ id: "agents::persist" }, async (data: any) => {
  const logger = new Logger();
  const userKey = `${data.sessionId}:user:${Date.now()}`;
  const assistantKey = `${data.sessionId}:assistant:${Date.now()}`;
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "chat-memory",
      key: userKey,
      value: { _key: userKey, sessionId: data.sessionId, role: "user", content: data.input },
    },
  });
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "chat-memory",
      key: assistantKey,
      value: { _key: assistantKey, sessionId: data.sessionId, role: "assistant", content: data.answer },
    },
  });
  logger.info("agents.persist.completed", { sessionId: data.sessionId });
  return { ok: true };
});

iii.registerFunction({ id: "agents::chat" }, async (request: any) => {
  const logger = new Logger();
  const sessionId = request.body.sessionId ?? `session-${Date.now()}`;
  const input = String(request.body.input ?? "");
  const memory = await iii.trigger({
    function_id: "state::list",
    payload: { scope: "chat-memory" },
  });
  const prior = memory.filter((m: any) => m.sessionId === sessionId).slice(-6).map((m: any) => m.content);
  const plan = await iii.trigger({
    function_id: "agents::plan",
    payload: { sessionId, input },
  });
  const retrieval = plan.needsDocs
    ? await iii.trigger({ function_id: "agents::retrieve", payload: { sessionId, input } })
    : { docs: [] };
  const response = await iii.trigger({
    function_id: "agents::respond",
    payload: { sessionId, input: `${prior.join("\n")}\n${input}`.trim(), docs: retrieval.docs },
  });
  await iii.trigger({
    function_id: "agents::persist",
    payload: { sessionId, input, answer: response.answer },
  });
  logger.info("agents.chat.completed", { sessionId });
  return { sessionId, answer: response.answer };
});

iii.registerTrigger({
  type: "http",
  function_id: "agents::chat",
  config: { api_path: "/agents/chat", http_method: "POST" },
});
