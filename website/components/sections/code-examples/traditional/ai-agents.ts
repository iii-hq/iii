import express from "express";
import OpenAI from "openai";
import { createClient } from "redis";
import { StateGraph } from "@langchain/langgraph";
import pino from "pino";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { trace } from "@opentelemetry/api";

type AgentState = {
  sessionId: string;
  input: string;
  docs: string[];
  answer: string;
};

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: "ai-agents-traditional",
});
void telemetry.start();

const logger = pino({
  name: "ai-agents-traditional",
  level: process.env.LOG_LEVEL ?? "info",
});
const tracer = trace.getTracer("ai-agents-traditional");
const redis = createClient({
  url: process.env.REDIS_URL || "redis://localhost:6379",
});
const openai = new OpenAI({
  apiKey: process.env.OPENAI_API_KEY,
});
const app = express();
app.use(express.json());

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ event, data }),
  });
}

async function respondNode(state: AgentState): Promise<Partial<AgentState>> {
  const completion = await openai.chat.completions.create({
    model: "gpt-4o-mini",
    messages: [
      {
        role: "system",
        content: "You are a concise support assistant.",
      },
      {
        role: "user",
        content: `${state.docs.join("\n")}\n${state.input}`.trim(),
      },
    ],
    stream: false,
  });
  const answer = completion.choices[0]?.message?.content ?? "";
  // ...optional token streaming path...
  await sendCentralLog("agents.respond.completed", {
    sessionId: state.sessionId,
    outputLength: answer.length,
  });
  return { answer };
}

async function persistNode(state: AgentState): Promise<Partial<AgentState>> {
  await redis.rPush(`chat-memory:${state.sessionId}`, state.input, state.answer);
  await sendCentralLog("agents.persist.completed", {
    sessionId: state.sessionId,
  });
  return {};
}

const graph = new StateGraph<AgentState>({
  channels: {} as any,
})
  .addNode("respond", respondNode)
  .addNode("persist", persistNode)
  .addEdge("__start__", "respond")
  .addEdge("respond", "persist")
  .addEdge("persist", "__end__")
  .compile();

app.post("/agents/chat", async (req, res) => {
  const span = tracer.startSpan("agents.chat");
  const sessionId = req.body.sessionId ?? `session-${Date.now()}`;
  const docs = await redis.lRange("knowledge-base", 0, 2);
  // ...routing/planning tool calls...
  const result = await graph.invoke({
    sessionId,
    input: String(req.body.input ?? ""),
    docs,
    answer: "",
  });
  await sendCentralLog("agents.chat.completed", {
    sessionId,
  });
  span.end();
  res.json({ sessionId, answer: result.answer });
});

void redis.connect();
app.listen(3008);
