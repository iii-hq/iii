---
title: 'Adapter Configuration'
description: 'Configure distributed adapters for horizontal scaling in production'
---

<Callout type="warn">
**Deprecated** - The `@motiadev/adapter-*` packages and `motia.config.ts` adapter configuration have been removed. Adapters are now configured through `config.yaml` modules.
</Callout>

## Migration

Adapter configuration is now handled in `config.yaml`, where each iii module declares its own adapter. See [Adapters (Removed)](/docs/development-guide/adapters) for migration details.

### Example: Redis Adapters in config.yaml

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

See [Configuration](/docs/development-guide/motia-config) for the full reference, and the [Deployment Guide](/docs/deployment-guide) for production examples.
