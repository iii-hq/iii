# iii Developer Console

A monochromatic developer and operations console for the **iii engine**. Built with Next.js 14+ and featuring real-time data streaming via iii's native WebSocket streams.

## Features

### 📊 Dashboard

The dashboard provides a real-time overview of your iii engine application.

**Top Metrics (with Mini Charts)**
- **Functions** 🟢 - Total registered functions in your app. Graph shows function count over time.
- **Triggers** 🟡 - Total active triggers (API, cron, events). Graph shows trigger count over time.
- **Workers** 🔵 - Number of active worker processes. Graph shows worker count fluctuations.
- **Uptime** 🟣 - How long the engine has been running. Graph shows uptime progression.

Each metric displays:
- Current value (large number)
- Trend percentage (↑ or ↓ with %)
- Mini sparkline chart showing last ~50 data points
- Color-coded by type

**System Status**
- **Version** - Engine version number (e.g., "0.0.0")
- **Metrics** - Total collected data points with "Live" indicator when streaming
- **Online/Offline Badge** - Green = engine connected, Red = engine offline
- **Live Indicator** - Shows WebSocket connection status for real-time updates

**Application Flow Diagram**
Visual representation of how your application works:

```
TRIGGERS → invoke → FUNCTIONS → r/w → STATES │ STREAMS
```

- **TRIGGERS** (Left Column)
  - 🔵 **REST API** - HTTP endpoints (GET, POST, DELETE, etc.)
  - 🟠 **SCHEDULED** - Cron jobs running on intervals
  - 🟣 **EVENTS** - Event listeners and handlers
  
- **FUNCTIONS** (Middle Column)
  - Shows all registered functions (max 6 + more indicator)
  - These are your business logic handlers
  
- **STATES** (Right Column)
  - 🔵 **KV STORE** - Key-value stores available to your functions
  - Shows active state groups
  
- **STREAMS** (Far Right)
  - 🟢 **WEBSOCKET** - Real-time bidirectional communication
  - Shows WebSocket port and connection status
  - ↓ In / ↑ Out indicators for data flow

**Legend (Bottom of Flow)**
- 🔵 API - HTTP/REST endpoints
- 🟠 Cron - Scheduled tasks
- 🟣 Events - Event-driven triggers
- 🔵 States - Persistent storage
- 🟢 Streams - WebSocket connections

**Registered Triggers Table**
Shows the most recent triggers (up to 4):
- **Type** - Badge showing trigger type (API, CRON, EVENT)
- **ID** - Unique trigger identifier (truncated)
- **Function** - Which function this trigger invokes
- **Status** - ACTIVE (green badge) when running

**Quick Actions Panel**
Direct navigation to key pages:
- **States** - Manage key-value data
- **Streams** - Monitor WebSocket traffic
- **Logs** - View application logs
- **Config** - Inspect configuration

**System Info Panel**
Runtime information:
- **Uptime** - How long engine has been running
- **API** - REST API port (default :3111)
- **WS** - WebSocket port (default :31112)
- **Updated** - Last metrics refresh timestamp

### 💾 States Page

**Purpose**: Manage persistent key-value data used by your functions.

**Layout**:
- **Left Sidebar** - List of state groups/stores
  - Click a group to view its items
  - Each group shows item count
  
- **Middle Panel** - Items in selected group
  - Table with columns: Key, Type, Preview
  - Sortable by key or type
  - Click item to view full details
  - Actions: Copy JSON, Delete item
  
- **Right Panel** - Item detail view
  - Full key name
  - Data type (string, number, object, array)
  - JSON value with syntax highlighting
  - Edit button (opens editor)
  - Delete button

**Use Cases**:
- Store user sessions
- Cache API responses
- Manage feature flags
- Track application state
- Store configuration data

### 🌊 Streams Page

**Purpose**: Monitor WebSocket traffic in real-time (like Chrome DevTools Network tab).

**Top Controls**:
- **Search** - Filter messages by content, stream name, or event type
- **Direction Filter** - All / Inbound / Outbound
- **Stream Filter** - Show only specific stream
- **Show System** - Toggle internal iii messages
- **Pause/Play** - Freeze/resume message capture
- **Clear** - Reset message list
- **Export** - Download as JSON

**Message List** (Main Area):
Each row shows:
- **Direction Icon**:
  - ↙️ Inbound (cyan) - Client → Server
  - ↗️ Outbound (green) - Server → Client  
  - ⚙️ System (gray) - Internal messages
- **Timestamp** - When message was sent/received
- **Stream Name** - Which stream this belongs to
- **Event Type** - create, update, delete, sync, subscribe
- **Data Preview** - First 100 chars of payload
- **Size** - Message size in bytes

**Click a Message** to see:
- Full timestamp
- Complete stream name
- Group ID (if applicable)
- Direction badge
- Full JSON data with syntax highlighting
- Copy button

**Stats Bar** (Bottom):
- Total messages captured
- Inbound count
- Outbound count
- Total bytes transferred
- Average latency (if available)

**Pagination** (Bottom):
- Page size selector (25, 50, 100, 250)
- Page navigation
- Shows "X-Y of Z" items

### ⚡ Functions & Triggers Page

**Purpose**: View all functions and their associated triggers in one place.

**Layout**:
- **Left Side** - List of all functions
  - Function path (e.g., `api.get.todo/:id`)
  - Trigger count badge
  - Click to see triggers
  
- **Right Panel** - Trigger details for selected function
  - Shows all triggers that invoke this function
  - Each trigger displays:
    - **Type Badge** - API, CRON, or EVENT
    - **Trigger ID** - Unique identifier
    - **Status** - ACTIVE badge
    - **Configuration**:
      - **API**: HTTP method, path, description
      - **CRON**: Schedule expression, next run time, description
      - **EVENT**: Event name, description
  
**Trigger Types Explained**:
- **API** 🔵 - REST endpoint that calls this function when hit
  - Shows: GET/POST/etc method and URL path
  - Manual test: Send button to trigger immediately
  
- **CRON** 🟠 - Scheduled task that runs periodically
  - Shows: Cron expression (e.g., "*/15 * * * *" = every 15 minutes)
  - Manual trigger: Run now button
  - Next run: Shows countdown to next execution
  
- **EVENT** 🟣 - Event listener that responds to emitted events
  - Shows: Event name it listens for
  - Manual emit: Test event emission

**Top Controls**:
- Search functions
- Filter by trigger type
- Show/hide system functions
- Function count badge

### 📝 Logs Page

**Purpose**: View and debug application logs in real-time.

**Top Controls**:
- **Search** - Filter by message content, trace ID, or source
- **Level Filters** - Click badges to filter:
  - 🟢 **DEBUG** - Detailed diagnostic info (gray dot)
  - 🔵 **INFO** - General informational messages (blue dot)
  - 🟡 **WARN** - Warning messages (yellow dot)
  - 🔴 **ERROR** - Error messages (red dot)
- **Auto-scroll** - Keeps showing latest logs
- **Clear** - Reset log list
- **Refresh** - Reload logs from server

**Log Entry Row**:
Each log displays:
- **Level Dot** - Color-coded circle (green/blue/yellow/red)
- **Timestamp** - When the log was created
- **Trace ID** - For distributed tracing (click to filter)
- **Source** - Which function/module created the log
- **Message** - The log message text
- **Context Badge** - Shows "+3" if additional data attached

**Click a Log** to see right panel with:
- Full timestamp (ISO format)
- Complete source path
- Full message (multiline if needed)
- Trace ID with link to traces page
- **Context Data** - JSON object with syntax highlighting
  - Shows all extra data attached to the log
  - Copy button to clipboard
  - Collapsible/expandable

**Double-Click a Log** for fullscreen view:
- Larger text
- Full screen JSON viewer
- Easier to read long messages or complex data
- Press Escape to close

**Pagination** (Bottom):
- Page size: 25, 50, 100, 250, 500
- Page navigator
- Total log count

**Stats Bar** (Bottom):
- Logs by source (top 3 sources)
- Total count per level
- Time range

### 🔍 Traces Page

**Purpose**: Distributed tracing for performance debugging (OpenTelemetry compatible).

**Status**: This page is ready for when you configure OpenTelemetry in your iii engine.

**What You'll See** (when configured):
- **Trace List** (Left) - All captured traces
  - Trace ID
  - Root operation name
  - Total duration
  - Status (success/error)
  - Number of spans
  
- **Trace Timeline** (Right) - Waterfall view
  - Visual representation of nested operations
  - Each span shows:
    - Operation name
    - Start time relative to trace start
    - Duration (width of bar)
    - Status color (green/red/yellow)
  
- **Span Details** (Click a span)
  - Full timing information
  - Tags and attributes
  - Events and logs within the span
  - Parent-child relationships

**How Traces Work**:
1. A request comes in (e.g., API call)
2. iii creates a trace ID
3. Each function call creates a span
4. Spans nest inside each other
5. Timeline shows where time was spent

**Use Cases**:
- Find slow database queries
- Identify bottlenecks
- Debug cascading function calls
- Track request flow across services

### ⚙️ Config Page

**Purpose**: Inspect engine configuration and available trigger types.

**Trigger Types** (Top Cards):
Shows all available ways to invoke functions:
- **API** 🔵 - HTTP/REST endpoints
  - Supports: GET, POST, PUT, DELETE, PATCH
  - Auto-routing based on function path
  
- **CRON** 🟠 - Scheduled tasks
  - Unix cron expressions (*/15 * * * *)
  - Background job execution
  
- **EVENT** 🟣 - Event-driven triggers
  - Subscribe to named events
  - Async message handling
  
- **STREAM** 🟢 - WebSocket triggers
  - Real-time data updates
  - Subscribe to stream changes

**Configuration Panel**:
- **View Config Button** - Opens modal with full config
  - Generated from runtime state
  - Shows all modules and their settings
  - **Copy** button to save config
  - **Close** to dismiss
  
**What's in the Config**:
```yaml
server:
  host: 0.0.0.0
  rest_api_port: 9001
  websocket_port: 31112
  management_api_port: 3111

modules:
  # Lists all enabled modules
  - class: modules::devtools::DevToolsModule
  - class: modules::rest_api::RestApiModule
  - class: modules::streams::StreamsModule
  - class: modules::observability::LoggingModule
  # ... etc
  
# Shows detected adapters
adapters:
  redis: connected
  logging: active
```

**Status Indicators**:
- 🟢 **Live** badge - Config is from running engine
- Module list with descriptions
- Adapter health status

## Understanding Your Data

### What Do the Numbers Mean?

**Functions Count (17)**
- Total number of registered functions in your application
- Includes: API handlers, cron jobs, event listeners
- Excludes: System functions (unless "Show System" enabled)
- Graph shows: How your codebase grows over time

**Triggers Count (17)**
- Total number of active triggers across all types
- Breakdown: API endpoints + Cron schedules + Event subscriptions
- Each trigger can invoke one or more functions
- Graph shows: Complexity of your application's entry points

**Workers Count (1)**
- Number of worker processes handling requests
- Default: 1 for single-process mode
- Higher numbers: Better concurrency for multi-core systems
- Graph shows: Worker availability and restarts

**Uptime**
- How long the engine has been running since last restart
- Format: "2m 41s" = 2 minutes, 41 seconds
- Longer uptime = stable deployment
- Resets to 0 when engine restarts

### What Do the Graphs Show?

**Mini Sparkline Charts**
- Each metric has a real-time graph
- X-axis: Time (last ~50 data points, ~25 minutes)
- Y-axis: Count or value
- Shows trends and patterns

**Reading the Graphs**:
- **Flat line** = Stable (good for uptime, workers)
- **Increasing** = Growth (adding functions/triggers)
- **Spikes** = Sudden changes (deployments, restarts)
- **Drops** = Removals or failures

**Trend Percentages**:
- 🟢 **↑ 37%** = Metric increased by 37%
- 🔴 **↓ 45%** = Metric decreased by 45%
- Compares recent average to older average

### Color Coding Guide

**Status Colors**:
- 🟢 **Green** - Success, Active, Online, Info logs
- 🟡 **Yellow** - Warning, Pending, Accent
- 🔴 **Red** - Error, Offline, Critical
- 🔵 **Blue** - API, States, Debug logs
- 🟣 **Purple** - Events, Uptime
- 🟠 **Orange** - Cron, Scheduled
- ⚪ **Gray** - System, Disabled, Debug

**Badge Colors**:
- **ACTIVE** (green) - Trigger is enabled and working
- **Type badges** - Color matches trigger type (API=cyan, CRON=orange, EVENT=purple)
- **Level badges** (logs) - Match severity (DEBUG=gray, INFO=blue, WARN=yellow, ERROR=red)

### Common Patterns

**Healthy Dashboard**:
- Functions & Triggers match or close
- Workers = 1 (or your expected count)
- Uptime increasing steadily
- Metrics graph shows data collection
- "Live" indicator green

**After Deployment**:
- Uptime resets to 0
- Function/Trigger counts may jump
- Workers restart (brief 0, then back to normal)
- New functions appear in Application Flow

**When Debugging**:
1. Check Logs page for errors (red dots)
2. Verify Functions page shows your handlers
3. Check States page for data issues
4. Use Streams to see real-time traffic
5. Traces page shows performance bottlenecks

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

Open [http://localhost:3113](http://localhost:3113)

### Default Ports

| Service | Port | Purpose |
|---------|------|---------|
| Console UI | 3113 | Next.js development server (3+ii+3) |
| DevTools API | 3111 | REST API endpoints (3+iii) |
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

## Quick Reference

### Page Navigation

| Page | Purpose | Key Features |
|------|---------|--------------|
| **Dashboard** | System overview | Metrics, application flow, quick stats |
| **States** | Key-value storage | Browse, edit, delete state data |
| **Streams** | WebSocket monitor | Real-time message inspection |
| **Functions** | Code registry | View all handlers and their triggers |
| **Logs** | Debug console | Filter, search, trace logs |
| **Traces** | Performance | Distributed tracing (when configured) |
| **Config** | Settings | View engine configuration |

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| **Escape** | Close modals/panels |
| **Double-click log** | Open fullscreen log viewer |
| **Click badge** | Toggle filter |

### Port Reference

| Port | Service | Meaning |
|------|---------|---------|
| **3113** | Console UI | 3 + ii + 3 (console) |
| **3111** | DevTools API | 3 + iii (engine) |
| **9001** | REST API (user apps) | User application |
| **31112** | WebSocket streams | 3 + iii + 2 (streams) |
| **6379** | Redis (optional) | Standard Redis port |

### API Endpoints

All DevTools endpoints are under `http://localhost:3111/_console/`:

| Endpoint | Returns |
|----------|---------|
| `/status` | System health and uptime |
| `/functions` | List of registered functions |
| `/triggers` | All active triggers |
| `/streams` | Available WebSocket streams |
| `/logs` | Application logs |
| `/metrics` | Current metrics snapshot |
| `/config` | Engine configuration |

### WebSocket Streams

Connect to: `ws://localhost:31112/streams/{stream_name}/{group_id}`

Example: `ws://localhost:31112/streams/iii:devtools:state/metrics`

### Troubleshooting Checklist

- [ ] Engine running? Check `cargo run` terminal
- [ ] DevTools enabled? Check `config.yaml`
- [ ] Ports available? Check nothing else uses 3111, 31112
- [ ] Redis running? Only needed for logs
- [ ] User app running? Run `iii-example` to see data
- [ ] Browser console? Look for connection errors

## License

Part of the iii engine project.
