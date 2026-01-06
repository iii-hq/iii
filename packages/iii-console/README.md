# iii Developer Console

A monochromatic developer and operations console for the **iii engine**. Built with Next.js 14+ and featuring real-time data streaming via iii's native WebSocket streams.

## Features

- **Dashboard** - System overview with real-time metrics (functions, workers, triggers, uptime)
- **Streams** - View and manage message queues and state
- **Triggers** - Monitor API, cron, and event triggers
- **Handlers** - View registered function handlers
- **Logs** - Debug system events and function invocations
- **Adapters** - Monitor connected components and health
- **Config** - Inspect engine configuration and environment

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
| `/_console/status` | GET | System status and metrics |
| `/_console/health` | GET | Health check |
| `/_console/functions` | GET | List registered functions |
| `/_console/triggers` | GET | List registered triggers |
| `/_console/trigger-types` | GET | List trigger types |
| `/_console/workers` | GET | List connected workers |
| `/_console/config` | GET | Engine configuration |
| `/_console/metrics` | GET | Current metrics snapshot |
| `/_console/metrics/history` | GET | Metrics history from stream |
| `/_console/events` | GET | Event subscription info |

## Real-time Updates

The console subscribes to iii Streams via WebSocket for live updates:

- **Stream**: `iii:devtools:state`
- **Group**: `metrics`
- **Events Topic**: `iii:devtools:events`

Metrics are collected every 30 seconds via a cron job and pushed to connected clients.

## Getting Started

### Prerequisites

1. **iii engine** running with DevTools module enabled
2. **Node.js 18+** installed

### Start the Engine

```bash
cd /path/to/iii-engine
cargo run
```

Verify DevTools is loaded:
```
[INITIALIZING] DevTools module on /_console/...
[READY] DevTools module ready - Events: iii:devtools:events, Stream: iii:devtools:state
```

### Start the Console

```bash
cd packages/iii-console
npm install
npm run dev
```

Open [http://localhost:3000](http://localhost:3000)

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
│   │   ├── page.tsx            # Dashboard
│   │   ├── streams/            # Streams view
│   │   ├── triggers/           # Triggers view
│   │   ├── handlers/           # Handlers view
│   │   ├── logs/               # Logs view
│   │   ├── adapters/           # Adapters view
│   │   └── config/             # Config view
│   ├── components/
│   │   ├── layout/
│   │   │   └── Sidebar.tsx     # Navigation sidebar
│   │   └── ui/
│   │       └── card.tsx        # UI components
│   └── lib/
│       └── api.ts              # API client with WebSocket streams
├── public/
├── tailwind.config.ts
└── package.json
```

## Tech Stack

- **Next.js 14** - React framework with App Router
- **Tailwind CSS** - Utility-first CSS
- **Lucide React** - Icon library
- **WebSocket** - Real-time streaming via iii Streams

## Development

```bash
# Install dependencies
npm install

# Run development server
npm run dev

# Build for production
npm run build

# Start production server
npm start
```

## License

Part of the iii engine project.
