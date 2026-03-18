import express from 'express';
import OpenAI from 'openai';
import { createClient } from 'redis';
import { StateGraph } from '@langchain/langgraph';
import pino from 'pino';
import { NodeSDK } from '@opentelemetry/sdk-node';
import { OTLPTraceExporter } from '@opentelemetry/exporter-trace-otlp-http';
import { trace } from '@opentelemetry/api';

type AgentState = {
  sessionId: string;
  input: string;
  history: string[];
  answer: string;
};

const telemetry = new NodeSDK({
  traceExporter: new OTLPTraceExporter({
    url: process.env.OTEL_EXPORTER_OTLP_TRACES_ENDPOINT,
  }),
  serviceName: 'ai-agents-traditional',
});
void telemetry.start();

const logger = pino({
  name: 'ai-agents-traditional',
  level: process.env.LOG_LEVEL ?? 'info',
});
const tracer = trace.getTracer('ai-agents-traditional');
const redis = createClient({
  url: process.env.REDIS_URL || 'redis://localhost:6379',
});
const openai = new OpenAI({
  apiKey: process.env.OPENAI_API_KEY,
});
const app = express();
app.use(express.json());

async function sendCentralLog(event: string, data: Record<string, unknown>) {
  logger.info({ event, ...data });
  await fetch(`${process.env.OBSERVABILITY_URL}/logs`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ event, data }),
  });
}

async function respondNode(state: AgentState): Promise<Partial<AgentState>> {
  const completion = await openai.chat.completions.create({
    model: 'gpt-4o-mini',
    messages: [
      {
        role: 'system',
        content: 'You are a concise support assistant.',
      },
      {
        role: 'user',
        content: state.input,
      },
    ],
    stream: false,
  });
  const answer = completion.choices[0]?.message?.content ?? '';
  await sendCentralLog('agents.respond.completed', {
    sessionId: state.sessionId,
    outputLength: answer.length,
  });
  return { answer };
}

const graph = new StateGraph<AgentState>({
  channels: {} as any,
})
  .addNode('respond', respondNode)
  .addEdge('__start__', 'respond')
  .addEdge('respond', '__end__')
  .compile();

app.post('/agents/chat', async (req, res) => {
  const span = tracer.startSpan('agents.chat');
  const sessionId = req.body.sessionId ?? `session-${Date.now()}`;
  const history = await redis.lRange(`chat-memory:${sessionId}`, 0, 5);
  const result = await graph.invoke({
    sessionId,
    input: String(req.body.input ?? ''),
    history,
    answer: '',
  });
  await sendCentralLog('agents.chat.completed', {
    sessionId,
  });
  span.end();
  res.json({ sessionId, answer: result.answer });
});

void redis.connect();
app.listen(3008);
