import { registerWorker, Logger } from 'iii-sdk';

const iii = registerWorker(
  process.env.III_ENGINE_URL || 'ws://localhost:49134',
  {
    workerName: 'ai-agents-iii',
  },
);

iii.registerFunction({ id: 'agents::chat' }, async (request: any) => {
  const logger = new Logger();
  const sessionId = request.body.sessionId ?? `session-${Date.now()}`;
  const input = String(request.body.input ?? '');
  const response = await iii.trigger({
    function_id: 'llm-service::respond',
    payload: {
      sessionId,
      model: 'gpt-4o-mini',
      messages: [
        {
          role: 'system',
          content: 'You are a concise support assistant.',
        },
        {
          role: 'user',
          content: input,
        },
      ],
    },
  });
  logger.info('agents.chat.completed', {
    sessionId,
  });
  return { sessionId, answer: response.answer };
});

iii.registerTrigger({
  type: 'http',
  function_id: 'agents::chat',
  config: {
    api_path: '/agents/chat',
    http_method: 'POST',
  },
});
