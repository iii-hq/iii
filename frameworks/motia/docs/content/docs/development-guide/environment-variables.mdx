---
title: Environment Variables
description: Store API keys and configuration safely using .env files in your Motia apps.
---

# Environment Variables

Environment variables let you store API keys, database URLs, and other configuration outside your code. This keeps sensitive information secure and makes it easy to use different settings for development and production.

## Quick Setup

### 1. Create a `.env` File

Create a `.env` file in your project root:

```bash title=".env"
# API Keys
OPENAI_API_KEY=sk-your-api-key-here
DISCORD_WEBHOOK_URL=https://discord.com/api/webhooks/your-webhook

# Database
DATABASE_URL=postgresql://user:password@localhost:5432/myapp

# App Settings
NODE_ENV=development
PORT=3000

# Redis (optional - only needed if using external Redis)
REDIS_URL=redis://localhost:6379
```

### 2. Add to `.gitignore`

Make sure you never commit your `.env` file:

```bash title=".gitignore"
.env
.env.local
```

### 3. Create Template for Your Team

```bash title=".env.example"
# Copy this to .env and add your actual values
OPENAI_API_KEY=your-api-key-here
DATABASE_URL=postgresql://user:password@localhost:5432/myapp
```

## Using Environment Variables in Steps

### TypeScript/JavaScript

```typescript title="my-step.step.ts"
import { type Handlers, type StepConfig, logger } from 'motia'

export const config = {
  name: 'chat-with-ai',
  description: 'Chat endpoint using OpenAI',
  triggers: [
    { type: 'http', path: '/chat', method: 'POST' },
  ],
  enqueues: [],
  flows: ['ai-chat'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  const apiKey = process.env.OPENAI_API_KEY
  const webhookUrl = process.env.DISCORD_WEBHOOK_URL

  if (!apiKey) {
    return { status: 400, body: { error: 'Missing API key' } }
  }

  logger.info('Using OpenAI API', { hasKey: !!apiKey })

  return { status: 200, body: { message: 'Success!' } }
}
```

### Python

```python title="my-step.step.py"
import os
from motia import logger

config = {
    "name": "process-data",
    "description": "Process incoming data",
    "triggers": [
        {"type": "queue", "topic": "data.received"}
    ],
    "flows": ["data-pipeline"]
}

async def handler(input_data):
    api_key = os.environ.get('OPENAI_API_KEY')
    database_url = os.environ.get('DATABASE_URL')

    if not api_key:
        raise ValueError('Missing OPENAI_API_KEY')

    logger.info('Processing with API key', {'has_key': bool(api_key)})

    return {'status': 'processed'}
```

## Redis Configuration

Motia uses Redis for internal coordination. By default, it includes an embedded in-memory server for development, so you don't need to install or configure Redis separately.

For production or when using an external Redis instance, configure Redis via your `config.yaml` file using the `REDIS_URL` environment variable:

```bash title=".env"
REDIS_URL=redis://redis.example.com:6379
```

```yaml title="config.yaml"
modules:
  - class: modules::state::StateModule
    config:
      adapter:
        class: modules::state::adapters::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}

  - class: modules::queue::QueueModule
    config:
      adapter:
        class: modules::queue::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}

  - class: modules::stream::StreamModule
    config:
      adapter:
        class: modules::stream::adapters::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}

  - class: modules::pubsub::PubSubModule
    config:
      adapter:
        class: modules::pubsub::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}
```

The `${REDIS_URL:redis://localhost:6379}` syntax uses environment variable interpolation with a default value. See the [Configuration](/docs/development-guide/motia-config) and [Deployment Guide](/docs/deployment-guide) for complete examples.

## Deployment

When you deploy your app, set environment variables through your hosting platform:

### Motia Cloud
```bash
motia env set OPENAI_API_KEY=sk-your-production-key
motia env set NODE_ENV=production
```

## Important Security Tips

<Callout type="warning">
**Keep Your Keys Safe**

- Never commit `.env` files to git
- Use different API keys for development and production
- Don't share API keys in code or messages
</Callout>

That's it! Environment variables are simple - just put them in `.env` and use `process.env.VARIABLE_NAME` in your code.
