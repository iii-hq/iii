# @motiadev/plugin-bullmq

A Motia workbench plugin for managing BullMQ queues and Dead Letter Queues (DLQ).

## Features

- **Queue Dashboard**: View all discovered queues with real-time statistics
- **Queue Operations**: Pause, resume, clean, and drain queues
- **Job Management**: View, retry, remove, and promote jobs
- **DLQ Management**: View failed jobs, retry from DLQ, bulk retry, and clear DLQ

## Installation

```bash
pnpm add @motiadev/plugin-bullmq
```

## Configuration

The plugin reads Redis connection configuration from environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `BULLMQ_REDIS_HOST` | `localhost` | Redis host |
| `BULLMQ_REDIS_PORT` | `6379` | Redis port |
| `BULLMQ_REDIS_PASSWORD` | - | Redis password (optional) |
| `BULLMQ_PREFIX` | `motia:events` | Queue prefix (matches Motia's default) |
| `BULLMQ_DLQ_SUFFIX` | `.dlq` | DLQ suffix pattern |

Alternatively, you can use the generic Redis variables:
- `REDIS_HOST`
- `REDIS_PORT`
- `REDIS_PASSWORD`

## Usage

Add the plugin to your Motia configuration:

```typescript
import bullmqPlugin from '@motiadev/plugin-bullmq/plugin'

export default {
  plugins: [bullmqPlugin],
}
```

## API Endpoints

### Queue Operations

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/__motia/bullmq/queues` | List all queues with stats |
| GET | `/__motia/bullmq/queues/:name` | Get queue details |
| POST | `/__motia/bullmq/queues/:name/pause` | Pause queue |
| POST | `/__motia/bullmq/queues/:name/resume` | Resume queue |
| POST | `/__motia/bullmq/queues/:name/clean` | Clean jobs by status |
| POST | `/__motia/bullmq/queues/:name/drain` | Drain queue |

### Job Operations

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/__motia/bullmq/queues/:name/jobs` | List jobs (query: status, start, end) |
| GET | `/__motia/bullmq/queues/:queueName/jobs/:jobId` | Get job details |
| POST | `/__motia/bullmq/queues/:queueName/jobs/:jobId/retry` | Retry job |
| POST | `/__motia/bullmq/queues/:queueName/jobs/:jobId/remove` | Remove job |
| POST | `/__motia/bullmq/queues/:queueName/jobs/:jobId/promote` | Promote delayed job |

### DLQ Operations

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/__motia/bullmq/dlq/:name/jobs` | List DLQ jobs |
| POST | `/__motia/bullmq/dlq/:name/retry/:jobId` | Retry job from DLQ |
| POST | `/__motia/bullmq/dlq/:name/retry-all` | Retry all jobs from DLQ |
| POST | `/__motia/bullmq/dlq/:name/clear` | Clear all DLQ jobs |

## Development

```bash
pnpm build
pnpm dev
```

