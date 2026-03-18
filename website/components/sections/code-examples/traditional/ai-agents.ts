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
  memory: string[];
  docs: string[];
  needsDocs: boolean;
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

function writeLog(
  level: "info" | "warn" | "error",
  payload: Record<string, unknown>,
) {
  if (level === "error") return logger.error(payload);
  if (level === "warn") return logger.warn(payload);
  return logger.info(payload);
}

async function sendCentralLog(
  level: "info" | "warn" | "error",
  event: string,
  data: Record<string, unknown>,
) {
  writeLog(level, { event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
    },
    body: JSON.stringify({
      service: "ai-agents-traditional",
      level,
      event,
      data,
      at: new Date().toISOString(),
    }),
  });
}

async function planNode(state: AgentState): Promise<Partial<AgentState>> {
  const needsDocs = /docs|policy|reference/i.test(state.input);
  await sendCentralLog("info", "agents.plan.completed", {
    sessionId: state.sessionId,
    needsDocs,
  });
  return { needsDocs };
}

async function retrieveNode(state: AgentState): Promise<Partial<AgentState>> {
  const stored = await redis.lRange("knowledge-base", 0, 2);
  const docs =
    stored.length > 0 ? stored : ["runbook", "api contract", "support policy"];
  await sendCentralLog("info", "agents.retrieve.completed", {
    sessionId: state.sessionId,
    count: docs.length,
  });
  return { docs };
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
        content:
          `${state.memory.join("\n")}\n${state.docs.join("\n")}\n${state.input}`.trim(),
      },
    ],
    stream: true,
  });
  let answer = "";
  for await (const chunk of completion) {
    const token = chunk.choices[0]?.delta?.content ?? "";
    if (token) {
      answer += token;
      await redis.publish(`agent-stream:${state.sessionId}`, token);
    }
  }
  await sendCentralLog("info", "agents.respond.completed", {
    sessionId: state.sessionId,
    outputLength: answer.length,
  });
  return { answer };
}

async function persistNode(state: AgentState): Promise<Partial<AgentState>> {
  await redis.rPush(
    `chat-memory:${state.sessionId}`,
    state.input,
    state.answer,
  );
  await redis.lTrim(`chat-memory:${state.sessionId}`, -12, -1);
  await sendCentralLog("info", "agents.persist.completed", {
    sessionId: state.sessionId,
  });
  return {};
}

const graph = new StateGraph<AgentState>({
  channels: {} as any,
})
  .addNode("plan", planNode)
  .addNode("retrieve", retrieveNode)
  .addNode("respond", respondNode)
  .addNode("persist", persistNode)
  .addEdge("__start__", "plan")
  .addConditionalEdges("plan", (state) =>
    state.needsDocs ? "retrieve" : "respond",
  )
  .addEdge("retrieve", "respond")
  .addEdge("respond", "persist")
  .addEdge("persist", "__end__")
  .compile();

app.post("/agents/chat", async (req, res) => {
  const span = tracer.startSpan("agents.chat");
  const sessionId = req.body.sessionId ?? `session-${Date.now()}`;
  const memory = await redis.lRange(`chat-memory:${sessionId}`, 0, -1);
  const result = await graph.invoke({
    sessionId,
    input: String(req.body.input ?? ""),
    memory,
    docs: [],
    needsDocs: false,
    answer: "",
  });
  await sendCentralLog("info", "agents.chat.completed", {
    sessionId,
  });
  span.end();
  res.json({ sessionId, answer: result.answer });
});

void redis.connect();
app.listen(3008);
