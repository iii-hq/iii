export interface CodeExample {
  traditional: {
    title: string;
    tools: string[];
    code: string;
    language: string;
  };
  iii: {
    title: string;
    code: string;
    language: string;
  };
  description: string;
  linesTraditional: number;
  linesIII: number;
}

export const codeExamples: Record<string, CodeExample> = {
  api: {
    description:
      "Build HTTP APIs without framework lock-in. One codebase, any language.",
    traditional: {
      title: "Express + Flask + FastAPI",
      tools: ["Express.js", "Flask", "FastAPI", "Koa", "Hono"],
      language: "typescript",
      code: `// Express.js setup - just for HTTP routing
import express from 'express'
import { createClient } from 'redis'
import Bull from 'bull'

const app = express()
const redis = createClient()
const queue = new Bull('tasks')

app.use(express.json())

// Manual route registration
app.post('/api/users', async (req, res) => {
  try {
    const user = await createUser(req.body)
    
    // Manually publish to Redis for other services
    await redis.publish('user.created', JSON.stringify(user))
    
    // Add background job manually
    await queue.add('sendWelcomeEmail', { userId: user.id })
    
    res.status(201).json(user)
  } catch (error) {
    res.status(500).json({ error: error.message })
  }
})

// Need separate Flask service for Python ML
// Need separate Rust service for performance-critical paths
// Each with its own framework, routing, and boilerplate

app.listen(3000)`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, Logger, TriggerAction } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

iii.registerFunction(
  { id: 'users::create' },
  async (req) => {
    const logger = new Logger()
    logger.info('Creating user', {
      email: req.body.email
    })

    const user = await createUser(req.body)

    iii.trigger({
      function_id: 'publish',
      payload: { topic: 'user.created', data: user },
      action: TriggerAction.Void()
    })

    return { status_code: 201, body: user }
  }
)

iii.registerTrigger({
  type: 'http',
  function_id: 'users::create',
  config: {
    api_path: 'users',
    http_method: 'POST'
  }
})`,
    },
    linesTraditional: 33,
    linesIII: 33,
  },

  jobs: {
    description:
      "Background jobs without Bull, Celery, or Sidekiq. Functions are the jobs.",
    traditional: {
      title: "Bull + Celery + Sidekiq",
      tools: ["Bull", "BullMQ", "Celery", "Sidekiq", "Agenda", "Dramatiq"],
      language: "typescript",
      code: `import Bull from 'bull'
import Redis from 'ioredis'

const redis = new Redis(process.env.REDIS_URL)
const emailQueue = new Bull('emails', { redis })
const reportQueue = new Bull('reports', { redis })

emailQueue.process('welcome', async (job) => {
  const { userId, email } = job.data
  await sendWelcomeEmail(email)
  return { sent: true }
})

emailQueue.on('failed', (job, err) => {
  if (job.attemptsMade < 3) {
    job.retry()
  } else {
    await deadLetterQueue.add(job.data)
  }
})

await emailQueue.add('welcome', { userId, email }, {
  attempts: 3,
  backoff: { type: 'exponential', delay: 1000 },
  removeOnComplete: true,
})`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, TriggerAction, Logger } from "iii-sdk"

const iii = registerWorker('ws://localhost:49134')

iii.registerFunction(
  { id: 'jobs::sendWelcomeEmail' },
  async (input) => {
    const logger = new Logger()
    logger.info('Sending email', { userId: input.userId })
    await sendWelcomeEmail(input.email)
    return { sent: true }
  }
)

await iii.trigger({
  function_id: 'jobs::sendWelcomeEmail',
  payload: { userId, email },
  action: TriggerAction.Enqueue({ queue: 'emails' }),
})`,
    },
    linesTraditional: 26,
    linesIII: 18,
  },

  events: {
    description:
      "Pub/Sub without RabbitMQ or Kafka. Events flow through the protocol.",
    traditional: {
      title: "Redis Pub/Sub + RabbitMQ",
      tools: ["Redis Pub/Sub", "RabbitMQ", "Kafka", "NATS", "AWS SQS"],
      language: "typescript",
      code: `// Redis Pub/Sub setup
import Redis from 'ioredis'

const publisher = new Redis(process.env.REDIS_URL)
const subscriber = new Redis(process.env.REDIS_URL)

// Separate connections for pub and sub
const subscriptions = new Map()

// Manual subscription management
async function subscribe(topic: string, handler: Function) {
  await subscriber.subscribe(topic)
  subscriptions.set(topic, handler)
}

subscriber.on('message', async (topic, message) => {
  const handler = subscriptions.get(topic)
  if (handler) {
    try {
      const data = JSON.parse(message)
      await handler(data)
    } catch (error) {
      console.error('Handler failed:', error)
      // Manual dead-letter logic
      await publisher.lpush('dead-letters', JSON.stringify({
        topic, message, error: error.message
      }))
    }
  }
})

// Publish events
async function publish(topic: string, data: any) {
  await publisher.publish(topic, JSON.stringify(data))
}

// Subscribe to events
await subscribe('user.created', async (user) => {
  await syncToCRM(user)
})

await subscribe('order.placed', async (order) => {
  await updateInventory(order)
  await notifyWarehouse(order)
})

// Need guaranteed delivery? Add RabbitMQ
// Need replay? Add Kafka
// Each with its own setup and mental model`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, Logger, TriggerAction } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

iii.registerFunction(
  { id: 'events::user::created' },
  async (user) => {
    const logger = new Logger()
    logger.info('Syncing to CRM', {
      userId: user.id
    })
    await syncToCRM(user)
  }
)

iii.registerFunction(
  { id: 'events::order::placed' },
  async (order) => {
    await updateInventory(order)
    await notifyWarehouse(order)
  }
)

iii.registerTrigger({
  type: 'subscribe',
  function_id: 'events::user::created',
  config: { topic: 'user.created' }
})

iii.registerTrigger({
  type: 'subscribe',
  function_id: 'events::order::placed',
  config: { topic: 'order.placed' }
})

iii.trigger({
  function_id: 'publish',
  payload: { topic: 'user.created', data: newUser },
  action: TriggerAction.Void()
})`,
    },
    linesTraditional: 49,
    linesIII: 41,
  },

  realtime: {
    description:
      "WebSockets without Socket.io or Pusher. Streams are first-class.",
    traditional: {
      title: "Socket.io + Pusher",
      tools: ["Socket.io", "Pusher", "Ably", "Liveblocks", "PartyKit"],
      language: "typescript",
      code: `// Socket.io setup
import { Server } from 'socket.io'
import { createAdapter } from '@socket.io/redis-adapter'
import Redis from 'ioredis'

const pubClient = new Redis(process.env.REDIS_URL)
const subClient = pubClient.duplicate()

const io = new Server(httpServer, {
  cors: { origin: '*' },
  adapter: createAdapter(pubClient, subClient)
})

// Room management
const rooms = new Map<string, Set<string>>()

io.on('connection', (socket) => {
  const userId = socket.handshake.auth.userId
  
  socket.on('join-room', async (roomId) => {
    socket.join(roomId)
    
    // Track membership manually
    if (!rooms.has(roomId)) {
      rooms.set(roomId, new Set())
    }
    rooms.get(roomId).add(userId)
    
    // Broadcast presence
    io.to(roomId).emit('user-joined', { userId })
  })
  
  socket.on('message', async (data) => {
    const { roomId, content } = data
    
    // Save to database manually
    const message = await db.messages.create({ roomId, content, userId })
    
    // Broadcast to room
    io.to(roomId).emit('new-message', message)
  })
  
  socket.on('disconnect', () => {
    // Clean up room membership
    rooms.forEach((members, roomId) => {
      if (members.has(userId)) {
        members.delete(userId)
        io.to(roomId).emit('user-left', { userId })
      }
    })
  })
})`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, Logger } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

const rooms = new Map<string, Map<string, any>>()

iii.createStream('chat', {
  get: async ({ group_id, item_id }) =>
    rooms.get(group_id)?.get(item_id) ?? null,
  set: async ({ group_id, item_id, data }) => {
    if (!rooms.has(group_id))
      rooms.set(group_id, new Map())
    const old = rooms.get(group_id)!.get(item_id)
    rooms.get(group_id)!.set(item_id, data)
    return { old_value: old, new_value: data }
  },
  delete: async ({ group_id, item_id }) => {
    const old = rooms.get(group_id)?.get(item_id)
    rooms.get(group_id)?.delete(item_id)
    return { old_value: old }
  },
  list: async ({ group_id }) =>
    [...(rooms.get(group_id)?.values() ?? [])],
  listGroups: async () => [...rooms.keys()],
  update: async () => null,
})

iii.registerFunction(
  { id: 'chat::onJoin' },
  async ({ subscription_id, group_id }) => {
    const logger = new Logger()
    logger.info('Joined', { room: group_id })
  }
)
iii.registerTrigger({
  type: 'stream:join',
  function_id: 'chat::onJoin',
  config: { stream_name: 'chat' }
})

iii.registerFunction(
  { id: 'chat::send' },
  async ({ roomId, content, userId }) => {
    const msg = { id: crypto.randomUUID(), content, userId }
    await iii.trigger({
      function_id: 'stream::set',
      payload: {
        stream_name: 'chat',
        group_id: roomId,
        item_id: msg.id,
        data: msg
      }
    })
    return msg
  }
)

iii.registerFunction(
  { id: 'chat::history' },
  async ({ roomId }) => {
    return iii.trigger({
      function_id: 'stream::list',
      payload: { stream_name: 'chat', group_id: roomId }
    })
  }
)

iii.registerFunction(
  { id: 'chat::getMessage' },
  async ({ roomId, messageId }) => {
    return iii.trigger({
      function_id: 'stream::get',
      payload: {
        stream_name: 'chat',
        group_id: roomId,
        item_id: messageId
      }
    })
  }
)`,
    },
    linesTraditional: 52,
    linesIII: 76,
  },

  state: {
    description:
      "Shared state without direct Redis. State is a first-class module.",
    traditional: {
      title: "Redis + Memcached",
      tools: ["Redis", "Memcached", "DynamoDB", "Upstash"],
      language: "typescript",
      code: `// Redis state management
import Redis from 'ioredis'

const redis = new Redis(process.env.REDIS_URL)

// Manual key management
const STATE_PREFIX = 'state:'
const SESSION_PREFIX = 'session:'
const CACHE_PREFIX = 'cache:'

async function getState(workflowId: string, key: string) {
  const data = await redis.hget(\`\${STATE_PREFIX}\${workflowId}\`, key)
  return data ? JSON.parse(data) : null
}

async function setState(workflowId: string, key: string, value: any) {
  await redis.hset(
    \`\${STATE_PREFIX}\${workflowId}\`,
    key,
    JSON.stringify(value)
  )
}

async function deleteState(workflowId: string, key: string) {
  await redis.hdel(\`\${STATE_PREFIX}\${workflowId}\`, key)
}

async function clearWorkflowState(workflowId: string) {
  const keys = await redis.hkeys(\`\${STATE_PREFIX}\${workflowId}\`)
  if (keys.length > 0) {
    await redis.hdel(\`\${STATE_PREFIX}\${workflowId}\`, ...keys)
  }
}

// Session management - separate logic
async function getSession(sessionId: string) {
  const data = await redis.get(\`\${SESSION_PREFIX}\${sessionId}\`)
  return data ? JSON.parse(data) : null
}

async function setSession(sessionId: string, data: any, ttl: number) {
  await redis.setex(
    \`\${SESSION_PREFIX}\${sessionId}\`,
    ttl,
    JSON.stringify(data)
  )
}

// Each service manages its own Redis connection
// No consistency across services
// No trace correlation`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, Logger, TriggerAction } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

iii.registerFunction(
  { id: 'workflow::process' },
  async (input) => {
    const logger = new Logger()

    const step = await iii.trigger({
      function_id: 'state::get',
      payload: {
        scope: input.workflowId,
        key: 'currentStep'
      }
    })

    logger.info('Processing', { step })

    await iii.trigger({
      function_id: 'state::set',
      payload: {
        scope: input.workflowId,
        key: 'currentStep',
        value: step + 1
      }
    })

    if (step < 5) {
      iii.trigger({
        function_id: 'workflow::process',
        payload: { workflowId: input.workflowId },
        action: TriggerAction.Void()
      })
    }

    return { step, status: 'processed' }
  }
)`,
    },
    linesTraditional: 51,
    linesIII: 33,
  },

  cron: {
    description:
      "Scheduled tasks without node-cron or Agenda. Cron is a trigger type.",
    traditional: {
      title: "node-cron + Agenda",
      tools: ["node-cron", "Agenda", "AWS EventBridge", "Cloud Scheduler"],
      language: "typescript",
      code: `// node-cron + Agenda setup
import cron from 'node-cron'
import Agenda from 'agenda'
import Redis from 'ioredis'

const redis = new Redis(process.env.REDIS_URL)
const agenda = new Agenda({ db: { address: process.env.MONGO_URL } })

// Simple cron - no distributed locking
cron.schedule('0 9 * * *', async () => {
  console.log('Running daily report...')
  await generateDailyReport()
})

// Agenda for distributed - needs MongoDB
agenda.define('send-weekly-digest', async (job) => {
  const { userId } = job.attrs.data
  await sendWeeklyDigest(userId)
})

// Manual distributed locking for node-cron
cron.schedule('*/5 * * * *', async () => {
  const lockKey = 'cron:cleanup:lock'
  const locked = await redis.set(lockKey, '1', 'NX', 'EX', 300)
  
  if (!locked) {
    console.log('Another instance running cleanup')
    return
  }
  
  try {
    await cleanupExpiredSessions()
  } finally {
    await redis.del(lockKey)
  }
})

// Schedule jobs
await agenda.start()
await agenda.every('1 week', 'send-weekly-digest', { userId: 123 })

// Different syntax for each library
// Manual locking for distributed scenarios
// No unified observability`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, Logger } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

iii.registerFunction(
  { id: 'reports::daily' },
  async () => {
    const logger = new Logger()
    logger.info('Generating daily report')
    const report = await generateDailyReport()
    await iii.trigger({
      function_id: 'state::set',
      payload: {
        scope: 'reports',
        key: 'daily-' + new Date()
          .toISOString().split('T')[0],
        value: report
      }
    })
    return { generated: true }
  }
)
iii.registerTrigger({
  type: 'cron',
  function_id: 'reports::daily',
  config: { expression: '0 9 * * *' }
})

iii.registerFunction(
  { id: 'maintenance::cleanup' },
  async () => {
    await cleanupExpiredSessions()
  }
)
iii.registerTrigger({
  type: 'cron',
  function_id: 'maintenance::cleanup',
  config: { expression: '*/5 * * * *' }
})`,
    },
    linesTraditional: 44,
    linesIII: 38,
  },

  logging: {
    description:
      "Observability without Datadog setup. Logging flows through the protocol.",
    traditional: {
      title: "Winston + Pino + Manual",
      tools: ["Winston", "Pino", "Bunyan", "OpenTelemetry", "Datadog SDK"],
      language: "typescript",
      code: `// Winston + OpenTelemetry setup
import winston from 'winston'
import { NodeSDK } from '@opentelemetry/sdk-node'
import { trace, context, SpanStatusCode } from '@opentelemetry/api'

// Configure Winston
const logger = winston.createLogger({
  level: 'info',
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.json()
  ),
  transports: [
    new winston.transports.Console(),
    new winston.transports.File({ filename: 'app.log' }),
  ],
})

// Configure OpenTelemetry
const sdk = new NodeSDK({
  serviceName: 'my-service',
  // ... lots of configuration
})
sdk.start()

const tracer = trace.getTracer('my-service')

// Manual trace propagation
async function handleRequest(req: Request) {
  const span = tracer.startSpan('handleRequest')
  const ctx = trace.setSpan(context.active(), span)
  
  return context.with(ctx, async () => {
    try {
      const traceId = span.spanContext().traceId
      
      // Log with trace correlation - manually
      logger.info('Processing request', {
        traceId,
        path: req.path,
        method: req.method,
      })
      
      const result = await processRequest(req)
      
      span.setStatus({ code: SpanStatusCode.OK })
      return result
    } catch (error) {
      span.setStatus({ code: SpanStatusCode.ERROR })
      span.recordException(error)
      logger.error('Request failed', { error: error.message })
      throw error
    } finally {
      span.end()
    }
  })
}`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, Logger } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

iii.registerFunction(
  { id: 'orders::process' },
  async (input) => {
    const logger = new Logger()

    logger.info('Processing order', {
      orderId: input.orderId,
      items: input.items.length
    })

    try {
      const order = await processOrder(input)
      logger.info('Order processed', {
        orderId: order.id,
        total: order.total
      })
      return order
    } catch (error) {
      logger.error('Order failed', {
        orderId: input.orderId,
        error: error.message
      })
      throw error
    }
  }
)`,
    },
    linesTraditional: 57,
    linesIII: 32,
  },

  workflow: {
    description:
      "Durable multi-step workflows without Temporal servers, Inngest DSL, or trigger.dev wrappers. State = durability. Queues = retries.",
    traditional: {
      title: "Temporal + Inngest + trigger.dev",
      tools: [
        "Temporal",
        "Inngest",
        "trigger.dev",
        "Cadence",
        "AWS Step Functions",
      ],
      language: "typescript",
      code: `// Temporal — the standard for durable execution
import { proxyActivities, sleep, ApplicationFailure } from '@temporalio/workflow'
import { Worker, NativeConnection } from '@temporalio/worker'
import type * as activities from './activities'

// All activities wrapped in proxyActivities — workflow must be deterministic
// No Date.now(), Math.random(), or direct DB/HTTP calls in workflow code
const { sendConfirmation, chargeCard, shipOrder } = proxyActivities<
  typeof activities
>({
  startToCloseTimeout: '30 seconds',
  retry: { maximumAttempts: 3, backoffCoefficient: 2 },
})

export async function orderWorkflow(order: Order): Promise<OrderResult> {
  await sendConfirmation({ email: order.email, orderId: order.id })

  const payment = await chargeCard({
    amount: order.total,
    cardToken: order.paymentToken,
  })

  if (!payment.success) {
    throw ApplicationFailure.nonRetryable('Payment declined')
  }

  // Pause workflow — replay-safe delay
  await sleep('30 seconds')

  const shipment = await shipOrder({ orderId: order.id, address: order.address })

  return { orderId: order.id, trackingNumber: shipment.trackingNumber }
}

// Separate worker process — must deploy, scale, and monitor independently
const worker = await Worker.create({
  connection: await NativeConnection.connect({ address: 'temporal:7233' }),
  taskQueue: 'orders',
  workflowsPath: require.resolve('./workflows'),
  activities,
})
await worker.run()

// Temporal Cloud: $490+/month or self-host Temporal server
// Inngest: step.run() wraps every activity + cloud HTTP endpoint required
// trigger.dev: task.run() + server-side SDK + hosted infrastructure
// All: vendor DSL + vendor infrastructure + deterministic code constraints`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, TriggerAction } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

iii.registerFunction(
  { id: 'order::start' },
  async (order) => {
    await sendConfirmation(order)
    await iii.trigger({
      function_id: 'state::set',
      payload: { scope: order.id, key: 'status', value: 'confirmed' }
    })
    await iii.trigger({
      function_id: 'order::charge',
      payload: order,
      action: TriggerAction.Enqueue({ queue: 'order::charge' })
    })
  }
)
iii.registerTrigger({
  type: 'http', function_id: 'order::start',
  config: { api_path: 'orders', http_method: 'POST' }
})

iii.registerFunction(
  { id: 'order::charge' },
  async (order) => {
    const pay = await chargeCard(order)
    if (!pay.success) return
    await iii.trigger({
      function_id: 'state::set',
      payload: { scope: order.id, key: 'status', value: 'charged' }
    })
    await iii.trigger({
      function_id: 'order::ship',
      payload: order,
      action: TriggerAction.Enqueue({ queue: 'order::ship' })
    })
  }
)

iii.registerFunction(
  { id: 'order::ship' },
  async (order) => {
    const s = await shipOrder(order)
    await iii.trigger({
      function_id: 'state::set',
      payload: { scope: order.id, key: 'status', value: 'shipped' }
    })
    return { tracking: s.trackingNumber }
  }
)`,
    },
    linesTraditional: 47,
    linesIII: 55,
  },

  "ai-agents": {
    description:
      "Build AI agent runtimes without LangChain complexity. Functions as tools, State for memory, Streams for responses.",
    traditional: {
      title: "LangChain + LangGraph + Redis",
      tools: ["LangChain", "LangGraph", "LlamaIndex", "AutoGen", "CrewAI"],
      language: "typescript",
      code: `// LangChain agent setup with tools
import { ChatOpenAI } from '@langchain/openai'
import { AgentExecutor, createOpenAIToolsAgent } from 'langchain/agents'
import { DynamicTool } from '@langchain/core/tools'
import { ChatPromptTemplate } from '@langchain/core/prompts'
import Redis from 'ioredis'

const redis = new Redis()
const model = new ChatOpenAI({ modelName: 'gpt-4' })

// Define tools manually
const tools = [
  new DynamicTool({
    name: 'search_database',
    description: 'Search the product database',
    func: async (query: string) => {
      const results = await db.products.search(query)
      return JSON.stringify(results)
    },
  }),
  new DynamicTool({
    name: 'send_email',
    description: 'Send an email to user',
    func: async (params: string) => {
      const { to, subject, body } = JSON.parse(params)
      await sendEmail(to, subject, body)
      return 'Email sent'
    },
  }),
]

// Memory management manually
async function getConversationHistory(sessionId: string) {
  const history = await redis.lrange(\`chat:\${sessionId}\`, 0, -1)
  return history.map(h => JSON.parse(h))
}

async function saveMessage(sessionId: string, msg: any) {
  await redis.rpush(\`chat:\${sessionId}\`, JSON.stringify(msg))
  await redis.expire(\`chat:\${sessionId}\`, 3600)
}

// Create agent with prompt
const prompt = ChatPromptTemplate.fromMessages([/* ... */])
const agent = await createOpenAIToolsAgent({ llm: model, tools, prompt })
const executor = new AgentExecutor({ agent, tools })

// Execute with streaming... complex setup
const stream = await executor.streamEvents(
  { input: userMessage },
  { version: 'v1' }
)`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, TriggerAction } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

iii.registerFunction(
  { id: 'tools::webSearch' },
  async ({ query }) => {
    return await searchWeb(query)
  }
)

iii.registerFunction(
  { id: 'agents::researcher' },
  async ({ topic }) => {
    const sources = await iii.trigger({
      function_id: 'tools::webSearch',
      payload: { query: topic }
    })
    return iii.trigger({
      function_id: 'agents::analyzer',
      payload: { sources, topic }
    })
  }
)

iii.registerFunction(
  { id: 'agents::analyzer' },
  async ({ sources, topic }) => {
    const draft = await callLLM(
      'Analyze sources', { sources }
    )
    await iii.trigger({
      function_id: 'state::set',
      payload: { scope: 'reports', key: topic, value: draft }
    })
    iii.trigger({
      function_id: 'publish',
      payload: { topic: 'report.ready', data: { topic } },
      action: TriggerAction.Void()
    })
  }
)

iii.registerFunction(
  { id: 'agents::writer' },
  async ({ topic }) => {
    const draft = await iii.trigger({
      function_id: 'state::get',
      payload: { scope: 'reports', key: topic }
    })
    return callLLM('Write final report', { draft })
  }
)
iii.registerTrigger({
  type: 'subscribe',
  function_id: 'agents::writer',
  config: { topic: 'report.ready' }
})`,
    },
    linesTraditional: 52,
    linesIII: 58,
  },

  "feature-flags": {
    description:
      "Real-time feature flags without LaunchDarkly or Split. State + Streams = instant propagation.",
    traditional: {
      title: "LaunchDarkly + Redis",
      tools: ["LaunchDarkly", "Split.io", "Unleash", "Flagsmith", "ConfigCat"],
      language: "typescript",
      code: `// LaunchDarkly setup
import * as LaunchDarkly from 'launchdarkly-node-server-sdk'
import Redis from 'ioredis'

const ldClient = LaunchDarkly.init(process.env.LD_SDK_KEY!)
const redis = new Redis()
const flagCache = new Map<string, any>()

// Wait for initialization
await ldClient.waitForInitialization()

// Subscribe to flag changes
ldClient.on('update', async (settings) => {
  // Manually sync to Redis for other services
  for (const [key, value] of Object.entries(settings)) {
    await redis.set(\`flag:\${key}\`, JSON.stringify(value))
    flagCache.set(key, value)
  }
  
  // Notify connected clients manually
  pubsub.publish('flag-updates', JSON.stringify(settings))
})

// Evaluate flag for user
async function getFlag(flagKey: string, user: User, defaultValue: any) {
  const ldUser = {
    key: user.id,
    email: user.email,
    custom: {
      plan: user.plan,
      createdAt: user.createdAt,
    },
  }
  
  const value = await ldClient.variation(flagKey, ldUser, defaultValue)
  
  // Track analytics manually
  await analytics.track('flag_evaluated', {
    flag: flagKey,
    value,
    userId: user.id,
  })
  
  return value
}

// Cleanup
process.on('SIGTERM', () => {
  ldClient.close()
})

// $25k+/year for enterprise features
// Vendor lock-in for flag definitions`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, Logger, TriggerAction } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

iii.registerFunction(
  { id: 'flags::set' },
  async ({ flagKey, config }) => {
    const logger = new Logger()
    await iii.trigger({
      function_id: 'state::set',
      payload: { scope: 'flags', key: flagKey, value: config }
    })
    iii.trigger({
      function_id: 'publish',
      payload: { topic: 'flags', data: { flag: flagKey, config } },
      action: TriggerAction.Void()
    })
    logger.info('Flag updated', { flagKey })
    return { updated: true }
  }
)

iii.registerFunction(
  { id: 'flags::evaluate' },
  async ({ flagKey, user, defaultValue }) => {
    const config = await iii.trigger({
      function_id: 'state::get',
      payload: { scope: 'flags', key: flagKey }
    })
    if (!config) return defaultValue
    if (config.userIds?.includes(user.id))
      return config.value
    if (config.percentage) {
      const hash = hashUser(user.id, flagKey)
      if (hash < config.percentage)
        return config.value
    }
    if (config.plans?.includes(user.plan))
      return config.value
    return defaultValue
  }
)`,
    },
    linesTraditional: 53,
    linesIII: 43,
  },

  multiplayer: {
    description:
      "Build multiplayer game backends without Photon or PlayFab. Streams for state, Events for actions.",
    traditional: {
      title: "Photon + PlayFab + Redis",
      tools: ["Photon", "PlayFab", "Nakama", "Colyseus", "Mirror"],
      language: "typescript",
      code: `// Colyseus game room setup
import { Room, Client } from 'colyseus'
import { Schema, MapSchema, type } from '@colyseus/schema'
import Redis from 'ioredis'

class Player extends Schema {
  @type('number') x: number = 0
  @type('number') y: number = 0
  @type('number') score: number = 0
}

class GameState extends Schema {
  @type({ map: Player }) players = new MapSchema<Player>()
}

class GameRoom extends Room<GameState> {
  private redis = new Redis()
  
  onCreate(options: any) {
    this.setState(new GameState())
    this.setSimulationInterval(() => this.update())
    
    // Handle messages
    this.onMessage('move', (client, data) => {
      const player = this.state.players.get(client.sessionId)
      if (player) {
        player.x = data.x
        player.y = data.y
      }
    })
    
    this.onMessage('action', async (client, data) => {
      // Process action, update score
      const player = this.state.players.get(client.sessionId)
      player.score += data.points
      
      // Persist to Redis
      await this.redis.hset(
        \`game:\${this.roomId}:scores\`,
        client.sessionId,
        player.score
      )
    })
  }
  
  onJoin(client: Client) {
    this.state.players.set(client.sessionId, new Player())
    this.broadcast('playerJoined', { id: client.sessionId })
  }
  
  onLeave(client: Client) {
    this.state.players.delete(client.sessionId)
    this.broadcast('playerLeft', { id: client.sessionId })
  }
  
  update() {
    // Game loop logic
  }
}

// Need separate matchmaking service
// Need separate leaderboard service`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, TriggerAction } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

const players = new Map<string, Map<string, any>>()
iii.createStream('game', {
  get: async ({ group_id, item_id }) =>
    players.get(group_id)?.get(item_id) ?? null,
  set: async ({ group_id, item_id, data }) => {
    if (!players.has(group_id))
      players.set(group_id, new Map())
    const old = players.get(group_id)!.get(item_id)
    players.get(group_id)!.set(item_id, data)
    return { old_value: old, new_value: data }
  },
  delete: async ({ group_id, item_id }) => {
    const old = players.get(group_id)?.get(item_id)
    players.get(group_id)?.delete(item_id)
    return { old_value: old }
  },
  list: async ({ group_id }) =>
    [...(players.get(group_id)?.values() ?? [])],
  listGroups: async () => [...players.keys()],
  update: async () => null,
})

iii.registerFunction(
  { id: 'game::onJoin' },
  async ({ subscription_id, group_id }) => {
    await iii.trigger({
      function_id: 'stream::set',
      payload: {
        stream_name: 'game',
        group_id,
        item_id: subscription_id,
        data: { x: 0, y: 0, score: 0 }
      }
    })
  }
)
iii.registerTrigger({
  type: 'stream:join',
  function_id: 'game::onJoin',
  config: { stream_name: 'game' }
})

iii.registerFunction(
  { id: 'game::move' },
  async ({ roomId, playerId, x, y }) => {
    const p = await iii.trigger({
      function_id: 'stream::get',
      payload: {
        stream_name: 'game',
        group_id: roomId,
        item_id: playerId
      }
    })
    await iii.trigger({
      function_id: 'stream::set',
      payload: {
        stream_name: 'game',
        group_id: roomId,
        item_id: playerId,
        data: { ...p, x, y }
      }
    })
  }
)

iii.registerFunction(
  { id: 'game::action' },
  async ({ roomId, playerId, points }) => {
    const p = await iii.trigger({
      function_id: 'stream::get',
      payload: {
        stream_name: 'game',
        group_id: roomId,
        item_id: playerId
      }
    })
    await iii.trigger({
      function_id: 'stream::set',
      payload: {
        stream_name: 'game',
        group_id: roomId,
        item_id: playerId,
        data: { ...p, score: p.score + points }
      }
    })
    iii.trigger({
      function_id: 'publish',
      payload: {
        topic: 'leaderboard',
        data: { playerId, score: p.score + points }
      },
      action: TriggerAction.Void()
    })
  }
)`,
    },
    linesTraditional: 62,
    linesIII: 82,
  },

  etl: {
    description:
      "Build ETL pipelines without cron jobs and manual checkpointing. Events for data flow, State for recovery points.",
    traditional: {
      title: "node-cron + Redis + Manual Workers",
      tools: ["node-cron", "Airflow", "Dagster", "Prefect", "Luigi"],
      language: "typescript",
      code: `// Manual ETL pipeline — node-cron + Redis checkpoints
import cron from 'node-cron'
import Redis from 'ioredis'

const redis = new Redis(process.env.REDIS_URL)

async function extractUsers() {
  const checkpoint = await redis.get('etl:checkpoint')
  return db.users.find({
    updated_at: { $gt: checkpoint ? new Date(checkpoint) : new Date(0) }
  })
}

// No stage isolation — one big function, fail = restart everything
cron.schedule('0 2 * * *', async () => {
  try {
    // Extract
    const users = await extractUsers()

    // Transform
    const transformed = users.map(user => ({
      user_id: user.id,
      lifetime_value: calculateLTV(user),
      segment: classifySegment(user),
    }))

    // Load
    await warehouse.bulkInsert('user_analytics', transformed)

    // Save checkpoint — only after full success
    await redis.set('etl:checkpoint', new Date().toISOString())
    console.log(\`ETL complete: \${transformed.length} records\`)
  } catch (err) {
    console.error('ETL failed — full re-run required:', err)
    // No retry per stage. No dead-letter. No visibility.
    // If cron overlaps — duplicate runs, race condition on checkpoint
  }
})

// Need: Redis + cron process + separate worker machines
// No per-stage observability, no partial recovery, no backfill`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, TriggerAction } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

iii.registerFunction(
  { id: 'etl::extract' },
  async ({ pipeline }) => {
    const cp = await iii.trigger({
      function_id: 'state::get',
      payload: { scope: pipeline, key: 'checkpoint' }
    })
    const users = await db.users.find({
      updated_at: { $gt: cp || new Date(0) }
    })
    iii.trigger({
      function_id: 'etl::transform',
      payload: { pipeline, users },
      action: TriggerAction.Void()
    })
  }
)

iii.registerFunction(
  { id: 'etl::transform' },
  async ({ pipeline, users }) => {
    const rows = users.map(u => ({
      user_id: u.id,
      ltv: calculateLTV(u),
      segment: classifySegment(u),
    }))
    iii.trigger({
      function_id: 'etl::load',
      payload: { pipeline, data: rows },
      action: TriggerAction.Void()
    })
  }
)

iii.registerFunction(
  { id: 'etl::load' },
  async ({ pipeline, data }) => {
    await warehouse.bulkInsert('user_analytics', data)
    await iii.trigger({
      function_id: 'state::set',
      payload: {
        scope: pipeline,
        key: 'checkpoint',
        value: new Date().toISOString()
      }
    })
  }
)

iii.registerTrigger({
  type: 'cron',
  function_id: 'etl::extract',
  config: { expression: '0 2 * * *' }
})`,
    },
    linesTraditional: 41,
    linesIII: 52,
  },

  reactive: {
    description:
      "Reactive backends without WebSocket servers or Redis wiring. Publish once, all subscribers update.",
    traditional: {
      title: "WebSocket + Redis + Postgres",
      tools: ["ws", "Socket.io", "Redis Pub/Sub", "Ably", "Supabase Realtime"],
      language: "typescript",
      code: `import { WebSocketServer } from 'ws'
import Redis from 'ioredis'

const wss = new WebSocketServer({ port: 8080 })
const pub = new Redis(process.env.REDIS_URL)
const sub = new Redis(process.env.REDIS_URL)

const channelSubs = new Map<string, Set<WebSocket>>()

sub.subscribe('messages')
sub.on('message', (_, payload) => {
  const { channelId, data } = JSON.parse(payload)
  channelSubs.get(channelId)?.forEach(ws => {
    if (ws.readyState === 1) ws.send(JSON.stringify(data))
  })
})

wss.on('connection', (ws, req) => {
  const channelId = new URL(req.url!, 'ws://x').searchParams.get('channel')!
  if (!channelSubs.has(channelId)) channelSubs.set(channelId, new Set())
  channelSubs.get(channelId)!.add(ws)

  ws.on('close', () => channelSubs.get(channelId)?.delete(ws))
})

app.post('/messages', async (req, res) => {
  const { channelId, content } = req.body
  await pub.publish('messages', JSON.stringify({ channelId, data: content }))
  res.json({ ok: true })
})`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, TriggerAction, Logger } from "iii-sdk"

const iii = registerWorker('ws://localhost:49134')

iii.registerFunction(
  { id: 'chat::send' },
  async ({ channelId, content }) => {
    const logger = new Logger()
    await db.messages.create({ channelId, content })

    iii.trigger({
      function_id: 'publish',
      payload: { topic: \`channel.\${channelId}\`, data: { content } },
      action: TriggerAction.Void(),
    })

    logger.info('Message sent', { channelId })
    return { ok: true }
  }
)

iii.registerFunction(
  { id: 'chat::onMessage' },
  async (data) => {
    const logger = new Logger()
    logger.info('New message received', data)
    return {}
  }
)

iii.registerTrigger({
  type: 'subscribe',
  function_id: 'chat::onMessage',
  config: { topic: 'channel.*' },
})`,
    },
    linesTraditional: 30,
    linesIII: 32,
  },

  remote: {
    description:
      "Route requests to external services — Lambda, Stripe, Cloud Functions — without glue code. iii as the universal router.",
    traditional: {
      title: "Express + axios + Manual Retries",
      tools: ["Express.js", "axios", "AWS SDK", "Stripe SDK", "p-retry"],
      language: "typescript",
      code: `// Express gateway to external services
import express from 'express'
import axios from 'axios'
import Stripe from 'stripe'
import { Lambda } from '@aws-sdk/client-lambda'

const app = express()
const stripe = new Stripe(process.env.STRIPE_KEY!)
const lambda = new Lambda({ region: 'us-east-1' })

// Manual retry logic
async function withRetry(fn: () => Promise<any>, retries = 3) {
  for (let i = 0; i < retries; i++) {
    try {
      return await fn()
    } catch (err) {
      if (i === retries - 1) throw err
      await new Promise(r => setTimeout(r, 1000 * Math.pow(2, i)))
    }
  }
}

// Route to Stripe
app.post('/api/payments', async (req, res) => {
  try {
    const session = await withRetry(() =>
      stripe.checkout.sessions.create({
        line_items: req.body.items,
        mode: 'payment',
        success_url: req.body.successUrl,
      })
    )
    res.json({ url: session.url })
  } catch (err) {
    res.status(500).json({ error: err.message })
  }
})

// Route to Lambda
app.post('/api/process', async (req, res) => {
  try {
    const result = await withRetry(() =>
      lambda.invoke({
        FunctionName: 'data-processor',
        Payload: JSON.stringify(req.body),
      })
    )
    res.json(JSON.parse(result.Payload as string))
  } catch (err) {
    res.status(502).json({ error: 'Upstream failed' })
  }
})

// Route to Google Cloud Function
app.post('/api/analyze', async (req, res) => {
  try {
    const { data } = await withRetry(() =>
      axios.post(process.env.GCF_URL!, req.body, {
        timeout: 30000,
        headers: { Authorization: \`Bearer \${process.env.GCF_TOKEN}\` },
      })
    )
    res.json(data)
  } catch (err) {
    res.status(502).json({ error: 'Upstream failed' })
  }
})

// Each external service: separate SDK, separate error handling
// No unified observability, no automatic retries across all
app.listen(3000)`,
    },
    iii: {
      title: "iii Engine",
      language: "typescript",
      code: `import { registerWorker, Logger } from "iii-sdk"

const iii = registerWorker(
  process.env.III_BRIDGE_URL ?? 'ws://localhost:49134'
)

iii.registerFunction(
  { id: 'remote::stripe::checkout' },
  async ({ items, successUrl }) => {
    const stripe = new Stripe(process.env.STRIPE_KEY!)
    const session = await stripe.checkout.sessions.create({
      line_items: items,
      mode: 'payment',
      success_url: successUrl,
    })
    return { url: session.url }
  }
)

iii.registerFunction(
  { id: 'remote::lambda::process' },
  async (payload) => {
    const lambda = new Lambda({ region: 'us-east-1' })
    const result = await lambda.invoke({
      FunctionName: 'data-processor',
      Payload: JSON.stringify(payload),
    })
    return JSON.parse(result.Payload as string)
  }
)

iii.registerFunction(
  { id: 'remote::gcf::analyze' },
  async (payload) => {
    const resp = await fetch(process.env.GCF_URL!, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: \`Bearer \${process.env.GCF_TOKEN}\`,
      },
      body: JSON.stringify(payload),
    })
    return await resp.json()
  }
)

iii.registerTrigger({
  type: 'http',
  function_id: 'remote::stripe::checkout',
  config: { api_path: 'payments', http_method: 'POST' }
})
iii.registerTrigger({
  type: 'http',
  function_id: 'remote::lambda::process',
  config: { api_path: 'process', http_method: 'POST' }
})
iii.registerTrigger({
  type: 'http',
  function_id: 'remote::gcf::analyze',
  config: { api_path: 'analyze', http_method: 'POST' }
})`,
    },
    linesTraditional: 71,
    linesIII: 60,
  },
};
