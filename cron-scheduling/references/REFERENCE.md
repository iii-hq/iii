# Cron Scheduling -- API Reference

Source: https://iii.dev/docs/how-to/schedule-cron-task

## Module Configuration

Enable the cron module in `iii-config.yaml`:

```yaml
modules:
  - class: modules::cron::CronModule
    config:
      adapter:
        class: modules::cron::KvCronAdapter
```

## Cron Expression Format

7-field format: `second minute hour day month weekday year`

| Schedule | Expression |
|----------|-----------|
| Every second | `* * * * * * *` |
| Every minute | `0 * * * * * *` |
| Every 5 minutes | `0 */5 * * * * *` |
| Every hour | `0 0 * * * * *` |
| Every 6 hours | `0 0 */6 * * * *` |
| Daily at midnight | `0 0 0 * * * *` |
| Daily at 9 AM | `0 0 9 * * * *` |
| Mondays at 9 AM | `0 0 9 * * 1 *` |
| Weekdays at 8 AM | `0 0 8 * * 1-5 *` |
| First of month midnight | `0 0 0 1 * * *` |
| Every 30 seconds | `*/30 * * * * * *` |

## Register a Function

### TypeScript/Node.js

```typescript
import { init, Logger } from 'iii-sdk'

const iii = init('ws://localhost:49134')
const logger = new Logger()

iii.registerFunction({ id: 'cleanup::expired-sessions' }, async () => {
  logger.info('Cleanup ran', { timestamp: new Date().toISOString() })
  // perform cleanup logic
  return { deleted: 0 }
})
```

### Python

```python
import os
from iii import init, Logger

iii = init(os.environ.get("III_URL", "ws://localhost:49134"))
logger = Logger()

def cleanup_sessions(_):
    logger.info("Cleanup ran")
    return {"deleted": 0}

iii.register_function({"id": "cleanup::expired-sessions"}, cleanup_sessions)
```

### Rust

```rust
use iii_sdk::{init, InitOptions, RegisterFunctionMessage, Logger};
use serde_json::json;

let iii = init("ws://127.0.0.1:49134", InitOptions::default())?;
let logger = Logger::new();

iii.register_function(
    RegisterFunctionMessage {
        id: "cleanup::expired-sessions".into(),
        ..Default::default()
    },
    |_| async move {
        logger.info("Cleanup ran", None);
        Ok(json!({ "deleted": 0 }))
    },
);
```

## Register a Cron Trigger

### TypeScript/Node.js

```typescript
iii.registerTrigger({
  type: 'cron',
  function_id: 'cleanup::expired-sessions',
  config: { expression: '0 0 * * * * *' }, // every hour
})
```

### Python

```python
iii.register_trigger({
    "type": "cron",
    "function_id": "cleanup::expired-sessions",
    "config": {"expression": "0 0 * * * * *"}  # every hour
})
```

### Rust

```rust
iii.register_trigger(RegisterTriggerInput {
    trigger_type: "cron".into(),
    function_id: "cleanup::expired-sessions".into(),
    config: json!({ "expression": "0 0 * * * * *" }),
})?;
```

## Complete Example: Daily Report Generator

```typescript
import { init, Logger, TriggerAction } from 'iii-sdk'

const iii = init('ws://localhost:49134')
const logger = new Logger()

// Generate and send daily report
iii.registerFunction({ id: 'reports::daily-summary' }, async () => {
  logger.info('Generating daily report')

  // Read aggregated state
  const stats = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'metrics', key: 'daily-totals' },
  })

  // Fire-and-forget email
  iii.trigger({
    function_id: 'notifications::send-report',
    payload: { stats: stats.value },
    action: TriggerAction.Void(),
  })

  return { generated: true }
})

// Run daily at 8 AM
iii.registerTrigger({
  type: 'cron',
  function_id: 'reports::daily-summary',
  config: { expression: '0 0 8 * * * *' },
})
```

## Cron with State Interaction

```typescript
iii.registerFunction({ id: 'metrics::aggregate-hourly' }, async () => {
  const raw = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'metrics', key: 'raw-events' },
  })

  const summary = processEvents(raw.value)

  await iii.trigger({
    function_id: 'state::set',
    payload: { scope: 'metrics', key: 'hourly-summary', value: summary },
  })

  return { aggregated: true }
})

iii.registerTrigger({
  type: 'cron',
  function_id: 'metrics::aggregate-hourly',
  config: { expression: '0 0 * * * * *' },
})
```
