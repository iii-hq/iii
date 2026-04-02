---
title: Adapters (Removed)
description: Adapters configuration has been removed from motia
---

<Callout type="error">
**Adapters have been removed from motia.** The adapters configuration that was previously available in `motia.config.ts` is no longer supported.
</Callout>

## What Changed

In previous versions, you could configure adapters for state, streams, events, and cron in `motia.config.ts`. This configuration has been removed. Adapter configuration is now handled through the `config.yaml` file, where each iii module declares its own adapter.

## Migration

Remove the `adapters` configuration from `motia.config.ts` and configure adapters in `config.yaml` modules instead. See [iii.dev/docs](https://iii.dev/docs) for the full module reference.

**Before (removed):**
```typescript
import { config } from '@motiadev/core'
import { RedisStateAdapter } from '@motiadev/adapter-redis-state'

export default config({
  adapters: {
    state: new RedisStateAdapter({ url: redisUrl })
  }
})
```

**After (`config.yaml`):**

```yaml
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
```

Each module in `config.yaml` configures its own adapter. See [Configuration](/docs/development-guide/motia-config) for details.

---
