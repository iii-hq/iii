import { traditional, iii } from './code-examples';

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
}

export const codeExamples: Record<string, CodeExample> = {
  api: {
    description:
      'Expose HTTP endpoints while keeping application logic portable across workers and runtimes.',
    traditional: {
      title: 'Express + Middleware + Validation',
      tools: ['Express.js', 'Zod'],
      language: 'typescript',
      code: traditional.api,
    },
    iii: {
      title: 'iii Worker + HTTP Triggers',
      language: 'typescript',
      code: iii.api,
    },
  },

  events: {
    description:
      'Publish and subscribe to events with first-class triggers instead of wiring brokers into app code.',
    traditional: {
      title: 'Broker + Publisher + Consumer',
      tools: ['RabbitMQ', 'amqplib'],
      language: 'typescript',
      code: traditional.pubsub,
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii.pubsub,
    },
  },

  realtime: {
    description:
      'Stream live updates from workers and state changes without a separate realtime integration layer.',
    traditional: {
      title: 'Socket.io + Presence',
      tools: ['Socket.io', 'Redis'],
      language: 'typescript',
      code: traditional.realtime,
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii.realtime,
    },
  },

  state: {
    description:
      'Use shared state with built-in reactions so reads, writes, and side effects stay in one model.',
    traditional: {
      title: 'Redis Cache + Store',
      tools: ['Redis'],
      language: 'typescript',
      code: traditional.state,
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii.state,
    },
  },

  cron: {
    description:
      'Schedule work with cron triggers that invoke the same functions used elsewhere in the system.',
    traditional: {
      title: 'node-cron + Scheduler',
      tools: ['node-cron'],
      language: 'typescript',
      code: traditional.cron,
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii.cron,
    },
  },

  logging: {
    description:
      'Keep logs and execution context tied to the same function and worker lifecycle.',
    traditional: {
      title: 'Logger + Tracing',
      tools: ['Pino', 'OpenTelemetry'],
      language: 'typescript',
      code: traditional.logging,
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii.logging,
    },
  },

  workflow: {
    description:
      'Compose durable multi-step workflows from functions, queues, and state when work spans retries or handoffs.',
    traditional: {
      title: 'Workflow Engine + Queue',
      tools: ['Temporal', 'Redis'],
      language: 'typescript',
      code: traditional.workflow,
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii.workflow,
    },
  },

  'ai-agents': {
    description:
      'Compose tools, memory, and streaming responses from the same functions, state, and worker primitives.',
    traditional: {
      title: 'Agent Framework + Memory',
      tools: ['LangGraph', 'OpenAI SDK', 'Redis'],
      language: 'typescript',
      code: traditional['ai-agents'],
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii['ai-agents'],
    },
  },

  'feature-flags': {
    description:
      'Model flag state centrally and react to changes in real time from workers or clients.',
    traditional: {
      title: 'Flag Service + Cache',
      tools: ['LaunchDarkly', 'Redis'],
      language: 'typescript',
      code: traditional['feature-flags'],
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii['feature-flags'],
    },
  },

  multiplayer: {
    description:
      'Coordinate rooms, player state, and live updates with shared state, events, and streams.',
    traditional: {
      title: 'Game Server + Realtime State',
      tools: ['Colyseus', 'Redis'],
      language: 'typescript',
      code: traditional.multiplayer,
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii.multiplayer,
    },
  },

  etl: {
    description:
      'Move data through scheduled or event-driven steps with shared progress and retryable handoffs.',
    traditional: {
      title: 'Scheduler + Workers',
      tools: ['node-cron', 'Redis'],
      language: 'typescript',
      code: traditional.etl,
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii.etl,
    },
  },

  reactive: {
    description:
      'Trigger downstream work from state changes instead of hand-wiring listeners across services.',
    traditional: {
      title: 'Change Feed + Subscribers',
      tools: ['Postgres', 'Redis Pub/Sub', 'ws'],
      language: 'typescript',
      code: traditional.reactive,
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii.reactive,
    },
  },

  remote: {
    description:
      'Register external HTTP services as functions so remote calls participate in the same trigger model.',
    traditional: {
      title: 'HTTP Client + Retries',
      tools: ['axios', 'p-retry'],
      language: 'typescript',
      code: traditional.remote,
    },
    iii: {
      title: 'iii Engine',
      language: 'typescript',
      code: iii.remote,
    },
  },
};
