# iii Developer Console

A monochromatic developer and operations console for the **iii engine**. Built with Next.js 14+ and featuring real-time data streaming via iii's native WebSocket streams.

## Features

### 📊 Dashboard
- Real-time system metrics (functions, workers, triggers, uptime)
- **Application Flow Visualization** - Visual diagram showing how triggers, functions, and streams connect
- Live WebSocket connection status
- Quick actions and recent trigger activity

### 🌊 Streams
- View all data streams and their state
- Real-time message counts and updates
- Filter system streams with "Show System" toggle

### ⚡ Triggers
- Monitor REST API, cron, and event triggers
- Expandable rows with full configuration
- View trigger-to-function mappings
- Copy trigger IDs and view full API URLs

### 🔧 Functions
- List all registered functions with paths
- Filter internal system functions
- View function execution stats
- Monitor active workers

### 📝 Logs
- Real-time log streaming from Redis adapter
- Filter by level (debug, info, warn, error)
- Search logs by message or trace ID
- Color-coded priority levels
- Context and metadata inspection

### 🔍 Traces (OpenTelemetry Ready)
- Prepared for distributed tracing with opentelemetry-rust
- Trace waterfall visualization
- Span details and timing analysis

### 🔌 Adapters
- View connected trigger types (API, cron, events, streams)
- Module health monitoring
- Real-time status indicators

### ⚙️ Config
- Inspect engine configuration
- View registered trigger types and endpoints
- Environment variable inspection (sensitive values hidden)

## Architecture

The console uses **iii's own capabilities** for its backend (dogfooding):

```
┌─────────────────────────────────────────────────────────────┐
│                    iii Developer Console                     │
│                      (Next.js Frontend)                      │
└─────────────────────┬───────────────────────────────────────┘
                      │
          ┌───────────┴───────────┐
          │                       │
          ▼                       ▼
┌─────────────────┐     ┌─────────────────┐
│   REST API      │     │   WebSocket     │
│   :3111         │     │   :31112        │
│   /_console/*   │     │   Streams       │
└────────┬────────┘     └────────┬────────┘
         │                       │
         └───────────┬───────────┘
                     │
         ┌───────────▼───────────┐
         │   DevTools Module     │
         │   (iii-native)        │
         │                       │
         │  • REST API Triggers  │
         │  • Cron Jobs          │
         │  • Event Emissions    │
         │  • Stream State       │
         └───────────────────────┘
```

## DevTools API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/_console/status` | GET | System status, uptime, and port info |
| `/_console/health` | GET | Health check endpoint |
| `/_console/functions` | GET | List all registered functions (with `internal` flag) |
| `/_console/triggers` | GET | List all triggers (API, cron, event, streams) |
| `/_console/trigger-types` | GET | List available trigger types |
| `/_console/workers` | GET | List connected workers and their stats |
| `/_console/streams` | GET | List all streams with message counts |
| `/_console/logs` | GET | Fetch logs from configured adapter (Redis/file) |
| `/_console/adapters` | GET | List connected adapters and modules |
| `/_console/config` | GET | Engine configuration and environment |
| `/_console/metrics` | GET | Current metrics snapshot |
| `/_console/metrics/history` | GET | Metrics history from stream state |
| `/_console/events` | GET | Event subscription information |

## Real-time Updates

The console subscribes to iii Streams via WebSocket for live updates:

- **Stream**: `iii:devtools:state`
- **Group**: `metrics`
- **WebSocket URL**: `ws://localhost:31112/streams/iii:devtools:state/metrics`

Metrics are collected every 30 seconds via a cron trigger and pushed to connected clients in real-time.

## User vs System Components

The console includes a **"Show System"** toggle on most pages to filter internal iii engine components:

- **User Components**: Your application's functions, triggers, streams (shown by default)
- **System Components**: Internal iii engine operations (devtools.*, core.*, logger.*, iii:devtools:*)

This helps SDK users focus on their application logic without seeing internal engine machinery. Advanced users can enable "Show System" to inspect engine internals.

## Getting Started

### Prerequisites

1. **iii engine** with DevTools module enabled in `config.yaml`
2. **Node.js 18+** and **pnpm** installed
3. **Redis** (optional, for log persistence)

### 1. Configure the Engine

Enable the DevTools module in `config.yaml`:

```yaml
modules:
  - class: modules::devtools::DevToolsModule
    config:
      api_prefix: "/_console"
      metrics_interval_seconds: 30
```

### 2. Configure Logging (Optional)

For persistent logs, enable the Redis adapter:

```yaml
modules:
  - class: modules::observability::LoggingModule
    config:
      level: debug
      format: default
      adapter:
        class: modules::observability::adapters::RedisLogger
        config:
          redis_url: redis://localhost:6379
```

### 3. Start the Engine

```bash
cd /path/to/iii-engine
cargo run
```

Verify DevTools is loaded:
```
[INITIALIZING] DevTools module on /_console/...
[READY] DevTools API on http://localhost:3111/_console/*
[READY] Streams on ws://localhost:31112
```

### 4. Run Example Application (Optional)

To see user components in the console:

```bash
cd packages/node/iii-example
pnpm install
pnpm start
```

This registers example functions, API triggers, and streams.

### 5. Start the Console

```bash
cd packages/iii-console
pnpm install
pnpm run dev
```

Open [http://localhost:3000](http://localhost:3000)

### Default Ports

| Service | Port | Purpose |
|---------|------|---------|
| Console UI | 3000 | Next.js development server |
| DevTools API | 3111 | REST API endpoints |
| REST API | 9001 | User application endpoints |
| Streams | 31112 | WebSocket connections |
| Redis | 6379 | Log storage (optional) |

## Brand Guidelines

The console follows iii's monochromatic brand:

| Color | Hex | Usage |
|-------|-----|-------|
| Black | `#0A0A0A` | Background |
| Dark Gray | `#141414` | Cards, sidebar |
| Medium Gray | `#1D1D1D` | Borders |
| Light Gray | `#F4F4F4` | Primary text |
| Muted | `#9CA3AF` | Secondary text |
| Yellow | `#F3F724` | Accent, highlights |
| Green | `#22C55E` | Success, online status |

**Typography**: JetBrains Mono (monospace)

## Project Structure

```
packages/iii-console/
├── src/
│   ├── app/                    # Next.js App Router pages
│   │   ├── page.tsx            # Dashboard with Application Flow
│   │   ├── streams/            # Streams management
│   │   ├── triggers/           # Triggers monitoring
│   │   ├── handlers/           # Functions view (renamed from handlers)
│   │   ├── logs/               # Logs with filtering
│   │   ├── traces/             # OpenTelemetry traces
│   │   ├── adapters/           # Adapter health
│   │   ├── config/             # Configuration inspector
│   │   ├── layout.tsx          # Root layout with font
│   │   └── globals.css         # Brand guidelines and styles
│   ├── components/
│   │   ├── layout/
│   │   │   └── Sidebar.tsx     # Navigation with iii logo
│   │   └── ui/
│   │       └── card.tsx        # Reusable UI components
│   └── lib/
│       └── api.ts              # API client + WebSocket streams
├── public/                     # Static assets
├── next.config.ts              # Next.js configuration
├── tailwind.config.ts          # Tailwind with brand colors
├── postcss.config.mjs          # PostCSS configuration
├── tsconfig.json               # TypeScript configuration
└── package.json                # Dependencies
```

## Tech Stack

- **Next.js 14** - React framework with App Router
- **Tailwind CSS** - Utility-first CSS
- **Lucide React** - Icon library
- **WebSocket** - Real-time streaming via iii Streams

## Development

```bash
# Install dependencies
pnpm install

# Run development server (with hot reload)
pnpm run dev

# Build for production
pnpm run build

# Start production server
pnpm start

# Lint code
pnpm run lint
```

## Troubleshooting

### Console shows "No user components registered"

**Solution**: Make sure you have an application registered using the iii SDK. Run the example app:
```bash
cd packages/node/iii-example && pnpm start
```

### Logs page shows "Configure a logging adapter"

**Solution**: Enable Redis logging in `config.yaml`:
```yaml
- class: modules::observability::LoggingModule
  config:
    adapter:
      class: modules::observability::adapters::RedisLogger
      config:
        redis_url: redis://localhost:6379
```

### WebSocket connection errors

**Solution**: 
1. Verify the iii engine is running on the expected ports
2. Check browser console for CORS issues
3. Ensure no other service is using port 31112

### Dashboard shows "Connecting..." indefinitely

**Solution**:
1. Verify DevTools module is enabled in `config.yaml`
2. Check that port 3111 is accessible: `curl http://localhost:3111/_console/status`
3. Ensure the Streams module is running on port 31112

### "Show System" toggle not working

**Solution**: This is expected if you only have system components. Register user functions via the SDK to see the difference.

## License

Part of the iii engine project.
