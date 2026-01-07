# iii Example - Todo App

A comprehensive example application demonstrating all iii engine capabilities. This Todo app showcases REST APIs, real-time streams, and structured logging.

## Features

- **REST APIs** - Full CRUD operations for todos
- **Streams** - Redis-backed persistent state management
- **Logging** - Structured logs visible in the iii Console
- **Real-time** - Live updates via WebSocket streams

## Prerequisites

1. **iii Engine** running with DevTools and Streams modules enabled
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

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/todos` | List all todos in inbox |
| GET | `/todo/:id` | Get a specific todo |
| POST | `/todo` | Create a new todo |
| PUT | `/todo/:id` | Update a todo |
| DELETE | `/todo/:id` | Delete a todo |
| POST | `/todo/:id/complete` | Mark a todo as complete |
| GET | `/stats` | Get todo statistics |
| GET | `/health` | Health check |

## Usage Examples

### Create a todo
```bash
curl -X POST http://localhost:3111/todo \
  -H "Content-Type: application/json" \
  -d '{"title": "Buy groceries", "priority": "high"}'
```

### List todos
```bash
curl http://localhost:3111/todos
```

### Complete a todo
```bash
curl -X POST http://localhost:3111/todo/{id}/complete
```

### Get statistics
```bash
curl http://localhost:3111/stats
```

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
├── index.ts      # API handlers with logging
├── hooks.ts      # useApi and other hooks
├── streams.ts    # Stream and logger helpers
├── types.ts      # TypeScript type definitions
└── bridge.ts     # SDK bridge configuration
```

## Viewing in iii Console

1. Start the iii Console: `cd ../iii-console && pnpm dev`
2. Open http://localhost:3000

You'll see:
- **Dashboard**: Overview of functions, triggers, and workers
- **Streams**: Your `todo` stream with inbox/completed groups
- **Functions**: All 8 registered API handlers
- **Triggers**: 8 API triggers for the todo endpoints
- **Logs**: Real-time logs from API operations

## License

Part of the iii engine project.

