# iii Example - Todo App

A comprehensive example application demonstrating **all** iii engine capabilities. This Todo app showcases REST APIs, event-driven workflows, real-time streams, cron jobs, and structured logging.

## Features

- **REST APIs** - Full CRUD operations for todos
- **Events** - Pub/sub messaging with event-driven workflows
- **Streams** - Redis-backed persistent state management
- **Cron Jobs** - Scheduled tasks (auto-archive, daily summaries)
- **Workflows** - Event chains for automation (auto-escalation, notifications)
- **Logging** - Structured logs visible in the iii Console
- **Real-time** - Live updates via WebSocket streams
- **Analytics** - Track completion stats and performance

## Prerequisites

1. **iii Engine** running with DevTools, Streams, and Event modules enabled
2. **Redis** running on `localhost:6379`
3. **Node.js 18+** installed

## Quick Start

```bash
# Install dependencies
pnpm install

# Start the app
pnpm start
# or
npx tsx src/index.ts
```

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           iii Engine                                     │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│   TRIGGERS                    FUNCTIONS                      OUTPUTS    │
│   ────────                    ─────────                      ───────    │
│                                                                          │
│   ┌──────────┐               ┌─────────────────┐         ┌───────────┐  │
│   │ REST API │──────────────▶│ Todo CRUD       │────────▶│ States    │  │
│   └──────────┘               └─────────────────┘         │ (Redis)   │  │
│                                       │                   └───────────┘  │
│   ┌──────────┐                       │                                   │
│   │ Cron     │──────────────▶ emit() │                   ┌───────────┐  │
│   └──────────┘                       ▼                   │ Streams   │  │
│                              ┌─────────────────┐         │ (WebSocket)│  │
│   ┌──────────┐               │ Event Handlers  │────────▶└───────────┘  │
│   │ Events   │◀──────────────│ (Workflows)     │                        │
│   └──────────┘               └─────────────────┘         ┌───────────┐  │
│                                       │                   │ Logs      │  │
│   ┌──────────┐                       │                   │ (Console) │  │
│   │ Stream   │◀──────────────────────┘                   └───────────┘  │
│   │ Events   │                                                           │
│   └──────────┘                                                           │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## API Endpoints

### Todo Management

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/todos` | List all todos (filter by `?list=inbox\|today\|completed`) |
| GET | `/todo/:id` | Get a specific todo |
| POST | `/todo` | Create a new todo |
| PUT | `/todo/:id` | Update a todo |
| DELETE | `/todo/:id` | Delete a todo |
| POST | `/todo/:id/complete` | Mark a todo as complete |
| GET | `/stats` | Get todo statistics |
| GET | `/health` | Health check |

### Analytics & Alerts

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/analytics/daily` | Get daily completion stats (`?date=YYYY-MM-DD`) |
| GET | `/alerts` | Get high priority alerts |
| POST | `/alert/:id/acknowledge` | Acknowledge an alert |

## Event Topics

The app emits and listens to the following events:

| Event | Trigger | Workflow Action |
|-------|---------|-----------------|
| `todo.created` | New todo created | Auto-escalate if contains urgent keywords |
| `todo.completed` | Todo marked complete | Update daily analytics |
| `todo.deleted` | Todo deleted | (available for extensions) |
| `todo.high_priority` | High priority detected | Store alert for notification |
| `todo.daily_summary` | Daily cron job | Generate summary statistics |

## Workflow Examples

### 1. Auto-Escalation Workflow
```
POST /todo {"title": "URGENT: Fix production bug"}
    │
    ▼
┌─────────────────────┐
│ todo.created event  │
└─────────────────────┘
    │
    ▼
┌─────────────────────────────────┐
│ Event Handler: Check keywords   │
│ - Detects "URGENT"              │
│ - Auto-escalates to high prio   │
└─────────────────────────────────┘
    │
    ▼
┌─────────────────────┐
│ todo.high_priority  │
│ event emitted       │
└─────────────────────┘
    │
    ▼
┌─────────────────────────────────┐
│ Alert stored in alerts stream   │
│ - Visible in /alerts endpoint   │
└─────────────────────────────────┘
```

### 2. Analytics Workflow
```
POST /todo/:id/complete
    │
    ▼
┌─────────────────────────┐
│ todo.completed event    │
└─────────────────────────┘
    │
    ▼
┌─────────────────────────────────┐
│ Event Handler: Update Stats     │
│ - Calculate completion time     │
│ - Update running average        │
│ - Store in analytics stream     │
└─────────────────────────────────┘
    │
    ▼
GET /analytics/daily → See stats!
```

## Cron Jobs

| Schedule | Description |
|----------|-------------|
| `*/15 * * * * *` | Archive completed todos older than 5 minutes |
| `0 0 * * *` | Generate daily summary at midnight |

## Usage Examples

### Create a todo
```bash
curl -X POST http://localhost:3111/todo \
  -H "Content-Type: application/json" \
  -d '{"title": "Buy groceries", "priority": "high"}'
```

### Create an urgent todo (triggers auto-escalation)
```bash
curl -X POST http://localhost:3111/todo \
  -H "Content-Type: application/json" \
  -d '{"title": "URGENT: Server is down!", "priority": "medium"}'
# This will auto-escalate to high priority due to "URGENT" keyword
```

### Complete a todo
```bash
curl -X POST http://localhost:3111/todo/{id}/complete
```

### View analytics
```bash
curl http://localhost:3111/analytics/daily
```

### View alerts
```bash
curl http://localhost:3111/alerts
```

### Acknowledge an alert
```bash
curl -X POST http://localhost:3111/alert/{alertId}/acknowledge
```

## State Stores

| Stream | Group | Contents |
|--------|-------|----------|
| `todo` | `inbox` | Pending todos |
| `todo` | `today` | Today's todos |
| `todo` | `completed` | Completed todos |
| `alerts` | `high_priority` | High priority alerts |
| `analytics` | `daily` | Daily statistics |
| `connections` | `active` | Active WebSocket connections |

## Configuration

The app uses `config.yaml` for iii engine configuration:

```yaml
modules:
  - class: modules::devtools::DevToolsModule
    config:
      api_prefix: "/_console"
      
  - class: modules::streams::StreamModule
    config:
      adapter:
        class: modules::streams::adapters::RedisAdapter
        config:
          redis_url: redis://localhost:6379
          
  - class: modules::event::EventModule
    config:
      adapter:
        class: modules::event::adapters::RedisAdapter
        config:
          redis_url: redis://localhost:6379
          
  - class: modules::observability::LoggingModule
    config:
      adapter:
        class: modules::observability::adapters::RedisLogger
        config:
          redis_url: redis://localhost:6379
```

## Project Structure

```
src/
├── index.ts      # All handlers: APIs, Events, Cron, Stream events
├── hooks.ts      # useApi, useEvent, useCron, useOnJoin, useOnLeave
├── streams.ts    # Stream, emit, and logger helpers
├── types.ts      # TypeScript type definitions
└── bridge.ts     # SDK bridge configuration
```

## Viewing in iii Console

1. Start the iii Console: `cd ../iii-console && pnpm dev`
2. Open http://localhost:3005

You'll see:
- **Dashboard**: Overview with application flow diagram
- **States**: Your `todo`, `alerts`, `analytics` stores
- **Streams**: Real-time WebSocket traffic
- **Triggers**: All API, Cron, and Event triggers
- **Functions**: All registered handlers
- **Logs**: Real-time logs from all operations

## Testing the Full Demo

```bash
# 1. Create some todos
curl -X POST http://localhost:3111/todo -H "Content-Type: application/json" \
  -d '{"title": "Normal task", "priority": "medium"}'

curl -X POST http://localhost:3111/todo -H "Content-Type: application/json" \
  -d '{"title": "URGENT: Fix this now!", "priority": "low"}'
  
# 2. Check alerts (should see auto-escalated alert)
curl http://localhost:3111/alerts

# 3. Complete a todo
curl -X POST http://localhost:3111/todo/{id}/complete

# 4. Check analytics
curl http://localhost:3111/analytics/daily

# 5. View everything in the Console at http://localhost:3005
```

## License

Part of the iii engine project.
