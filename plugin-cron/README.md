# @motiadev/cron-plugin

A Motia workbench plugin for monitoring and managing cron jobs (scheduled tasks).

## Overview

This plugin adds a "Cron Jobs" tab to the Motia workbench bottom panel, providing:

- Real-time cron job monitoring
- Human-readable cron expression descriptions
- Next/last execution time display
- Execution history with duration tracking
- Manual job triggering
- Enable/disable job controls
- Status filtering (idle, running, completed, failed, disabled)
- Search functionality

## Installation

```bash
npm install @motiadev/cron-plugin
# or
pnpm add @motiadev/cron-plugin
```

## Usage

Add the plugin to your `motia.config.ts`:

```typescript
import cronPlugin from '@motiadev/cron-plugin/plugin'

export default {
  plugins: [cronPlugin],
}
```

The plugin will automatically appear as a "Cron Jobs" tab in the bottom panel of your Motia workbench.

## Features

### Job Monitoring

- View all registered cron steps
- Real-time status updates via WebSocket
- Countdown timer to next execution

### Job Controls

- **Trigger**: Manually run a cron job
- **Enable/Disable**: Toggle job scheduling
- **View History**: See past executions with timing

### Execution Tracking

- Duration measurement for each run
- Error capturing and display
- Trace ID correlation

### Filtering

- Filter by status (idle, running, failed, etc.)
- Search by job name or description
- Filter by associated flow

## Exported APIs

```typescript
// Main component
export { CronJobsPage, CronJobCard } from '@motiadev/cron-plugin'

// Types
export type { CronJob, CronExecution, CronStats, CronFilter } from '@motiadev/cron-plugin'

// Zustand store
export { useCronStore } from '@motiadev/cron-plugin'

// Hooks
export { useCronMonitor, useCronJobs, useJobExecutions, useCronStats } from '@motiadev/cron-plugin'
```

## Development

```bash
pnpm install
pnpm run dev      # Watch mode
pnpm run build    # Production build
pnpm run clean    # Remove dist/
```

## Requirements

- Motia with `@motiadev/core` and `@motiadev/ui`
- React 19+

## License

MIT

