# RFC: Adapter Pattern for Horizontal Scaling

## Status
- **RFC Date**: 2025-10-15
- **Status**: Implemented
- **Authors**: Motia Team
- **Implementation Date**: 2025-10-20
- **Reviewers**: TBD

## Summary

This RFC proposes implementing a comprehensive adapter pattern for Motia that enables horizontal scaling by externalizing state management, stream processing, event handling, and cron job coordination to distributed systems like Redis, RabbitMQ, Kafka, and other cloud-native solutions. This will allow Motia applications to scale across multiple instances in production environments while maintaining data consistency, event delivery guarantees, and preventing duplicate cron job executions.

## Background

Currently, Motia provides four core components for application functionality:
- **State Management**: Stores application data across steps
- **Streams**: Manages data collections with real-time updates
- **Events**: Handles pub/sub messaging between steps
- **Cron Steps**: Executes scheduled tasks based on cron expressions

The current implementations use file-based and in-memory storage:
- **File Adapter**: Stores data in `.motia/*.json` files
- **Memory Adapter**: Stores data in process memory
- **Cron Scheduling**: Uses `node-cron` library for in-process scheduling

However, users deploying to production environments face critical limitations:
- **Single Instance Limitation**: File and memory adapters only work within a single process, making horizontal scaling impossible
- **No High Availability**: If the application instance crashes, all in-memory state and queued events are lost
- **No Distributed Events**: Events are only delivered within the same process, preventing distributed architectures
- **No Persistence Guarantees**: File writes are not atomic and can be corrupted during crashes
- **Cluster Incompatibility**: Cannot deploy to Kubernetes, ECS, or other container orchestration platforms with multiple replicas
- **Duplicate Cron Executions**: When running multiple instances, each instance schedules and executes the same cron jobs, leading to duplicate executions and potential data corruption

The documentation mentions Redis adapter support, but this is not actually implemented in the codebase.

## Goals

### Primary Goals

1. **Enable Horizontal Scaling**: Allow Motia applications to run multiple instances simultaneously with shared state and event distribution
2. **Pluggable Adapter System**: Define clear interfaces that allow users to implement custom adapters for any distributed system
3. **Production-Ready Implementations**: Provide battle-tested adapters for popular systems (Redis, RabbitMQ, Kafka)
4. **Backward Compatibility**: Ensure existing applications continue to work without changes
5. **Configuration-Based Selection**: Allow adapter selection through simple configuration files
6. **Zero Code Changes**: Enable scaling without modifying application code
7. **Type Safety**: Provide full TypeScript type support for all adapter implementations
8. **Cron Coordination**: Prevent duplicate cron job executions across multiple instances through distributed locking

### Secondary Goals

1. **Adapter Marketplace**: Create a registry of community-contributed adapters


### Non-Goals

- Building a new distributed database or message queue system
- Replacing existing well-established distributed systems
- Providing adapter implementations for every possible technology
- Supporting multi-cloud deployments in the initial implementation

## Architecture Overview

### High-Level System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      Motia Application                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              Motia Core Runtime                          │  │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────┐  │  │
│  │  │  State   │  │ Streams  │  │  Events  │  │  Cron  │  │  │
│  │  │ Manager  │  │ Manager  │  │ Manager  │  │Manager │  │  │
│  │  └────┬─────┘  └────┬─────┘  └────┬─────┘  └───┬────┘  │  │
│  │       │             │              │            │       │  │
│  │       v             v              v            v       │  │
│  │  ┌──────────────────────────────────────────────────┐  │  │
│  │  │         Adapter Interface Layer                  │  │  │
│  │  │  ┌────────┐ ┌────────┐ ┌────────┐ ┌──────────┐ │  │  │
│  │  │  │ State  │ │ Stream │ │ Event  │ │   Cron   │ │  │  │
│  │  │  │Adapter │ │Adapter │ │Adapter │ │ Adapter  │ │  │  │
│  │  │  └───┬────┘ └───┬────┘ └───┬────┘ └────┬─────┘ │  │  │
│  │  └──────┼──────────┼──────────┼───────────┼───────┘  │  │
│  └─────────┼──────────┼──────────┼───────────┼──────────┘  │
│            │          │          │           │             │
└────────────┼──────────┼──────────┼───────────┼─────────────┘
             │          │          │           │
             v          v          v           v
    ┌────────────────────────────────────────────────────────┐
    │      Distributed Infrastructure Layer                  │
    ├────────────────────────────────────────────────────────┤
    │                                                        │
    │  ┌────────┐  ┌────────┐  ┌──────────┐  ┌──────────┐ │
    │  │ Redis  │  │ Redis  │  │ RabbitMQ │  │  Redis   │ │
    │  │ State  │  │Streams │  │  Events  │  │Cron Locks│ │
    │  └────────┘  └────────┘  └──────────┘  └──────────┘ │
    │                                                        │
    │  ┌────────┐  ┌────────┐  ┌──────────┐  ┌──────────┐ │
    │  │DynamoDB│  │ Kafka  │  │ AWS SNS  │  │Postgres  │ │
    │  │ State  │  │Streams │  │  Events  │  │Cron Locks│ │
    │  └────────┘  └────────┘  └──────────┘  └──────────┘ │
    │                                                        │
    └────────────────────────────────────────────────────────┘
```

### Data Flow Architecture

```
Instance 1                  Instance 2                  Instance 3
┌──────────┐               ┌──────────┐               ┌──────────┐
│  Motia   │               │  Motia   │               │  Motia   │
│  App     │               │  App     │               │  App     │
└────┬─────┘               └────┬─────┘               └────┬─────┘
     │                          │                          │
     │                          │                          │
     v                          v                          v
┌─────────────────────────────────────────────────────────────┐
│                   State Adapter (Redis)                     │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐             │
│  │ Trace:123 │  │ Trace:456 │  │ Trace:789 │             │
│  │ state:foo │  │ state:bar │  │ state:baz │             │
│  └───────────┘  └───────────┘  └───────────┘             │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│              Event Adapter (RabbitMQ)                       │
│                                                             │
│  Exchange: motia.events                                    │
│  ├─ Queue: user.created  → [Instance 1, Instance 2, ...]  │
│  ├─ Queue: order.placed  → [Instance 1, Instance 3, ...]  │
│  └─ Queue: email.sent    → [All instances]                │
└─────────────────────────────────────────────────────────────┘
```

## Detailed Design

### 1. Adapter Interfaces

#### State Adapter Interface

```typescript
export interface StateAdapter {
  get<T>(traceId: string, key: string): Promise<T | null>
  
  set<T>(traceId: string, key: string, value: T): Promise<T>
  
  delete<T>(traceId: string, key: string): Promise<T | null>
  
  clear(traceId: string): Promise<void>
  
  cleanup(): Promise<void>
  
  keys(traceId: string): Promise<string[]>
  
  exists(traceId: string, key: string): Promise<boolean>
  
  getMany<T>(traceId: string, keys: string[]): Promise<(T | null)[]>
  
  setMany<T>(traceId: string, items: Record<string, T>): Promise<void>
}
```

#### Stream Adapter Interface

```typescript
export interface StreamAdapter<TData> {
  get(groupId: string, id: string): Promise<BaseStreamItem<TData> | null>
  
  set(groupId: string, id: string, data: TData): Promise<BaseStreamItem<TData>>
  
  delete(groupId: string, id: string): Promise<BaseStreamItem<TData> | null>
  
  getGroup(groupId: string): Promise<BaseStreamItem<TData>[]>
  
  send<T>(channel: StateStreamEventChannel, event: StateStreamEvent<T>): Promise<void>
  
  subscribe<T>(
    channel: StateStreamEventChannel,
    handler: (event: StateStreamEvent<T>) => void | Promise<void>
  ): Promise<void>
  
  unsubscribe(channel: StateStreamEventChannel): Promise<void>
  
  clear(groupId: string): Promise<void>
  
  query(groupId: string, filter: StreamQueryFilter<TData>): Promise<BaseStreamItem<TData>[]>
}

export interface StreamQueryFilter<TData> {
  limit?: number
  offset?: number
  orderBy?: keyof TData
  orderDirection?: 'asc' | 'desc'
  where?: Partial<TData>
}
```

#### Event Adapter Interface

```typescript
export interface EventAdapter {
  emit<TData>(event: Event<TData>): Promise<void>
  
  subscribe<TData>(
    topic: string,
    handler: (event: Event<TData>) => void | Promise<void>,
    options?: SubscribeOptions
  ): Promise<SubscriptionHandle>
  
  unsubscribe(handle: SubscriptionHandle): Promise<void>
  
  shutdown(): Promise<void>
  
  getSubscriptionCount(topic: string): Promise<number>
  
  listTopics(): Promise<string[]>
}

export interface SubscribeOptions {
  queue?: string
  exclusive?: boolean
  durable?: boolean
  prefetch?: number
}

export interface SubscriptionHandle {
  topic: string
  id: string
  unsubscribe: () => Promise<void>
}
```

#### Cron Adapter Interface

In a horizontally scaled environment, cron-steps present a unique challenge: each pod will attempt to schedule and execute the same cron jobs, leading to duplicate executions. This is problematic for tasks like sending scheduled emails, generating reports, or performing cleanup operations where duplicate execution could cause data inconsistency or unwanted side effects.

The Cron Adapter solves this by providing distributed coordination mechanisms to ensure that only one instance executes each scheduled job, even when multiple pods are running.

##### Problem Statement

When deploying Motia applications with multiple replicas:
- Each instance independently schedules cron jobs based on the same cron expressions
- All instances will trigger the same job at the same time
- This leads to duplicate executions, wasted resources, and potential data corruption
- No built-in mechanism exists to coordinate which instance should execute the job

##### Solution Approaches

There are three primary approaches to solving this problem:

1. **Distributed Locking**: Each instance attempts to acquire a lock before executing a cron job
2. **Leader Election**: Only one instance (the leader) schedules and executes cron jobs
3. **External Scheduler**: Use an external system designed for distributed cron scheduling

We recommend the **Distributed Locking** approach as it provides the best balance of simplicity, reliability, and fault tolerance.

##### Cron Adapter Interface

```typescript
export interface CronAdapter {
  acquireLock(jobName: string, ttl: number): Promise<CronLock | null>
  
  releaseLock(lock: CronLock): Promise<void>
  
  renewLock(lock: CronLock, ttl: number): Promise<boolean>
  
  isHealthy(): Promise<boolean>
  
  shutdown(): Promise<void>
  
  getActiveLocks(): Promise<CronLockInfo[]>
}

export interface CronLock {
  jobName: string
  lockId: string
  acquiredAt: number
  expiresAt: number
  instanceId: string
}

export interface CronLockInfo {
  jobName: string
  instanceId: string
  acquiredAt: number
  expiresAt: number
}
```

##### How It Works

1. **Job Scheduling**: All instances schedule cron jobs normally using their cron expressions
2. **Lock Acquisition**: When a cron job triggers, the instance attempts to acquire a distributed lock
3. **Execution**: Only the instance that successfully acquires the lock executes the job
4. **Lock Release**: After execution completes (or fails), the lock is released
5. **TTL Protection**: Locks have a TTL to prevent deadlocks if an instance crashes during execution

##### Execution Flow

```
Instance 1                  Instance 2                  Instance 3
    |                           |                           |
    | Cron triggers             | Cron triggers             | Cron triggers
    | (9:00 AM)                 | (9:00 AM)                 | (9:00 AM)
    |                           |                           |
    v                           v                           v
acquireLock("daily-report")  acquireLock("daily-report")  acquireLock("daily-report")
    |                           |                           |
    v                           v                           v
┌─────────────────────────────────────────────────────────────┐
│              Distributed Lock Store (Redis)                 │
│                                                             │
│  Lock: daily-report                                        │
│  Owner: instance-1                                         │
│  Acquired: 2025-10-20 09:00:00                            │
│  Expires: 2025-10-20 09:05:00                             │
└─────────────────────────────────────────────────────────────┘
    |                           |                           |
    v                           v                           v
Lock acquired ✓            Lock failed ✗              Lock failed ✗
    |                           |                           |
    v                           |                           |
Execute job                  Skip execution             Skip execution
    |                           |                           |
    v                           |                           |
releaseLock()                   |                           |
    |                           |                           |
    v                           v                           v
```

##### Configuration Options

```typescript
export interface CronAdapterConfig {
  lockTTL?: number
  lockRetryDelay?: number
  lockRetryAttempts?: number
  instanceId?: string
  enableHealthCheck?: boolean
}
```

- `lockTTL`: Time in milliseconds before a lock expires (default: 300000 = 5 minutes)
- `lockRetryDelay`: Delay between retry attempts if lock acquisition fails (default: 1000ms)
- `lockRetryAttempts`: Number of times to retry acquiring a lock (default: 0, no retries)
- `instanceId`: Unique identifier for this instance (default: auto-generated)
- `enableHealthCheck`: Whether to perform periodic health checks (default: true)

##### Edge Cases and Considerations

1. **Long-Running Jobs**: If a job takes longer than the lock TTL, the lock will expire and another instance might start executing the same job. Solution: Implement lock renewal for long-running jobs.

2. **Instance Crashes**: If an instance crashes while holding a lock, the lock will expire after the TTL, allowing another instance to execute the job. This provides automatic recovery.

3. **Clock Skew**: Different instances might have slightly different system clocks, causing cron jobs to trigger at slightly different times. This is acceptable as the lock mechanism will still ensure only one execution.

4. **Network Partitions**: If an instance loses connection to the lock store, it should fail to acquire locks and skip execution rather than risk duplicate execution.

5. **Lock Store Unavailability**: If the lock store (e.g., Redis) is unavailable, all instances will fail to acquire locks and no cron jobs will execute. This is a fail-safe behavior to prevent duplicates.

### 2. Configuration Schema

#### motia.config.ts Configuration

```typescript
import { config, type Config, type Motia } from '@motiadev/core'
import { RedisStateAdapter } from '@motiadev/adapter-redis-state'
import { RedisStreamAdapter } from '@motiadev/adapter-redis-streams'
import { RabbitMQEventAdapter } from '@motiadev/adapter-rabbitmq-events'
import { RedisCronAdapter } from '@motiadev/adapter-redis-cron'

export default config({
  adapters: {
    state: new RedisStateAdapter({
      host: process.env.REDIS_HOST || 'localhost',
      port: parseInt(process.env.REDIS_PORT || '6379'),
      password: process.env.REDIS_PASSWORD,
      keyPrefix: 'motia:state:',
      ttl: 3600,
    }),
    
    streams: new RedisStreamAdapter({
      host: process.env.REDIS_HOST || 'localhost',
      port: parseInt(process.env.REDIS_PORT || '6379'),
      password: process.env.REDIS_PASSWORD,
      keyPrefix: 'motia:stream:',
    }),
    
    events: new RabbitMQEventAdapter({
      url: process.env.RABBITMQ_URL || 'amqp://localhost',
      exchangeName: 'motia.events',
      exchangeType: 'topic',
      durable: true,
    }),
    
    cron: new RedisCronAdapter({
      host: process.env.REDIS_HOST || 'localhost',
      port: parseInt(process.env.REDIS_PORT || '6379'),
      password: process.env.REDIS_PASSWORD,
      keyPrefix: 'motia:cron:',
      lockTTL: 300000,
      instanceId: process.env.INSTANCE_ID || undefined,
    }),
  },
  
  plugins: [
  ],
})
```

#### Environment Variables

```bash
REDIS_HOST=redis.production.com
REDIS_PORT=6379
REDIS_PASSWORD=secure_password
REDIS_KEY_PREFIX=myapp:motia:

RABBITMQ_URL=amqp://user:pass@rabbitmq.production.com
RABBITMQ_EXCHANGE=motia.events

STATE_ADAPTER=redis
STREAM_ADAPTER=redis
EVENT_ADAPTER=rabbitmq
```

### 3. Core Type Updates

Update `Config` type in `packages/core/src/types/app-config-types.ts`:

```typescript
import type { Motia } from '../motia'
import type { StateAdapter } from '../state/state-adapter'
import type { StreamAdapter } from '../streams/adapters/stream-adapter'
import type { EventAdapter } from '../adapters/event-adapter'

export type Runtime = {
  steps: string
  streams: string
  runtime: any
}

export type WorkbenchPlugin = {
  packageName: string
  componentName?: string
  label?: string
  labelIcon?: string
  position?: 'bottom' | 'top'
  cssImports?: string[]
  props?: Record<string, any>
}

export type MotiaPlugin = {
  workbench: WorkbenchPlugin[]
}

export type MotiaPluginBuilder = (motia: Motia) => MotiaPlugin

export type AdapterConfig = {
  state?: StateAdapter
  streams?: StreamAdapter<any>
  events?: EventAdapter
  cron?: CronAdapter
}

export type Config = {
  runtimes?: Runtime[]
  plugins?: MotiaPluginBuilder[]
  adapters?: AdapterConfig
}
```

## Implementation Examples

### Example 1: Redis State Adapter

```typescript
import Redis from 'ioredis'
import type { StateAdapter } from '@motiadev/core'

export interface RedisStateAdapterConfig {
  host: string
  port: number
  password?: string
  db?: number
  keyPrefix?: string
  ttl?: number
}

export class RedisStateAdapter implements StateAdapter {
  private client: Redis
  private keyPrefix: string
  private ttl?: number

  constructor(config: RedisStateAdapterConfig) {
    this.client = new Redis({
      host: config.host,
      port: config.port,
      password: config.password,
      db: config.db || 0,
    })
    this.keyPrefix = config.keyPrefix || 'motia:state:'
    this.ttl = config.ttl
  }

  private makeKey(traceId: string, key: string): string {
    return `${this.keyPrefix}${traceId}:${key}`
  }

  async get<T>(traceId: string, key: string): Promise<T | null> {
    const fullKey = this.makeKey(traceId, key)
    const value = await this.client.get(fullKey)
    return value ? JSON.parse(value) : null
  }

  async set<T>(traceId: string, key: string, value: T): Promise<T> {
    const fullKey = this.makeKey(traceId, key)
    const serialized = JSON.stringify(value)
    
    if (this.ttl) {
      await this.client.setex(fullKey, this.ttl, serialized)
    } else {
      await this.client.set(fullKey, serialized)
    }
    
    return value
  }

  async delete<T>(traceId: string, key: string): Promise<T | null> {
    const fullKey = this.makeKey(traceId, key)
    const value = await this.get<T>(traceId, key)
    await this.client.del(fullKey)
    return value
  }

  async clear(traceId: string): Promise<void> {
    const pattern = `${this.keyPrefix}${traceId}:*`
    const keys = await this.client.keys(pattern)
    
    if (keys.length > 0) {
      await this.client.del(...keys)
    }
  }

  async cleanup(): Promise<void> {
    await this.client.quit()
  }

  async keys(traceId: string): Promise<string[]> {
    const pattern = `${this.keyPrefix}${traceId}:*`
    const keys = await this.client.keys(pattern)
    const prefixLength = this.makeKey(traceId, '').length
    return keys.map(key => key.slice(prefixLength))
  }

  async exists(traceId: string, key: string): Promise<boolean> {
    const fullKey = this.makeKey(traceId, key)
    const exists = await this.client.exists(fullKey)
    return exists === 1
  }

  async getMany<T>(traceId: string, keys: string[]): Promise<(T | null)[]> {
    if (keys.length === 0) return []
    
    const fullKeys = keys.map(key => this.makeKey(traceId, key))
    const values = await this.client.mget(...fullKeys)
    
    return values.map(value => value ? JSON.parse(value) : null)
  }

  async setMany<T>(traceId: string, items: Record<string, T>): Promise<void> {
    const pipeline = this.client.pipeline()
    
    for (const [key, value] of Object.entries(items)) {
      const fullKey = this.makeKey(traceId, key)
      const serialized = JSON.stringify(value)
      
      if (this.ttl) {
        pipeline.setex(fullKey, this.ttl, serialized)
      } else {
        pipeline.set(fullKey, serialized)
      }
    }
    
    await pipeline.exec()
  }
}
```

### Example 2: RabbitMQ Event Adapter

```typescript
import amqp, { type Connection, type Channel, type ConsumeMessage } from 'amqplib'
import type { EventAdapter, Event, SubscribeOptions, SubscriptionHandle } from '@motiadev/core'
import { v4 as uuidv4 } from 'uuid'

export interface RabbitMQEventAdapterConfig {
  url: string
  exchangeName: string
  exchangeType: 'direct' | 'topic' | 'fanout' | 'headers'
  durable?: boolean
  autoDelete?: boolean
}

export class RabbitMQEventAdapter implements EventAdapter {
  private connection?: Connection
  private channel?: Channel
  private config: RabbitMQEventAdapterConfig
  private subscriptions: Map<string, SubscriptionHandle> = new Map()

  constructor(config: RabbitMQEventAdapterConfig) {
    this.config = {
      durable: true,
      autoDelete: false,
      ...config,
    }
  }

  private async ensureConnection(): Promise<Channel> {
    if (!this.connection) {
      this.connection = await amqp.connect(this.config.url)
      this.channel = await this.connection.createChannel()
      
      await this.channel.assertExchange(
        this.config.exchangeName,
        this.config.exchangeType,
        {
          durable: this.config.durable,
          autoDelete: this.config.autoDelete,
        }
      )
    }
    
    return this.channel!
  }

  async emit<TData>(event: Event<TData>): Promise<void> {
    const channel = await this.ensureConnection()
    
    const message = {
      topic: event.topic,
      data: event.data,
      traceId: event.traceId,
      timestamp: Date.now(),
    }
    
    const content = Buffer.from(JSON.stringify(message))
    
    channel.publish(
      this.config.exchangeName,
      event.topic,
      content,
      {
        persistent: true,
        contentType: 'application/json',
      }
    )
  }

  async subscribe<TData>(
    topic: string,
    handler: (event: Event<TData>) => void | Promise<void>,
    options?: SubscribeOptions
  ): Promise<SubscriptionHandle> {
    const channel = await this.ensureConnection()
    const queueName = options?.queue || `motia.${topic}.${uuidv4()}`
    
    const queue = await channel.assertQueue(queueName, {
      durable: options?.durable ?? true,
      exclusive: options?.exclusive ?? false,
      autoDelete: !options?.durable,
    })
    
    await channel.bindQueue(queue.queue, this.config.exchangeName, topic)
    
    if (options?.prefetch) {
      await channel.prefetch(options.prefetch)
    }
    
    const consumerTag = await channel.consume(
      queue.queue,
      async (msg: ConsumeMessage | null) => {
        if (!msg) return
        
        try {
          const content = JSON.parse(msg.content.toString())
          await handler(content as Event<TData>)
          channel.ack(msg)
        } catch (error) {
          console.error('Error processing message:', error)
          channel.nack(msg, false, false)
        }
      }
    )
    
    const handle: SubscriptionHandle = {
      topic,
      id: consumerTag.consumerTag,
      unsubscribe: async () => {
        await this.unsubscribe(handle)
      },
    }
    
    this.subscriptions.set(handle.id, handle)
    return handle
  }

  async unsubscribe(handle: SubscriptionHandle): Promise<void> {
    const channel = await this.ensureConnection()
    await channel.cancel(handle.id)
    this.subscriptions.delete(handle.id)
  }

  async shutdown(): Promise<void> {
    if (this.channel) {
      await this.channel.close()
    }
    if (this.connection) {
      await this.connection.close()
    }
  }

  async getSubscriptionCount(topic: string): Promise<number> {
    return Array.from(this.subscriptions.values())
      .filter(sub => sub.topic === topic)
      .length
  }

  async listTopics(): Promise<string[]> {
    return Array.from(new Set(
      Array.from(this.subscriptions.values()).map(sub => sub.topic)
    ))
  }
}
```

### Example 3: Redis Streams Adapter

```typescript
import Redis from 'ioredis'
import type { StreamAdapter, BaseStreamItem, StateStreamEvent, StateStreamEventChannel } from '@motiadev/core'

export interface RedisStreamAdapterConfig {
  host: string
  port: number
  password?: string
  db?: number
  keyPrefix?: string
}

export class RedisStreamAdapter<TData> implements StreamAdapter<TData> {
  private client: Redis
  private keyPrefix: string
  private subscriptions: Map<string, NodeJS.Timeout> = new Map()

  constructor(config: RedisStreamAdapterConfig) {
    this.client = new Redis({
      host: config.host,
      port: config.port,
      password: config.password,
      db: config.db || 0,
    })
    this.keyPrefix = config.keyPrefix || 'motia:stream:'
  }

  private makeKey(groupId: string, id?: string): string {
    return id 
      ? `${this.keyPrefix}${groupId}:${id}`
      : `${this.keyPrefix}${groupId}`
  }

  async get(groupId: string, id: string): Promise<BaseStreamItem<TData> | null> {
    const key = this.makeKey(groupId, id)
    const value = await this.client.get(key)
    return value ? JSON.parse(value) : null
  }

  async set(groupId: string, id: string, data: TData): Promise<BaseStreamItem<TData>> {
    const key = this.makeKey(groupId, id)
    const item: BaseStreamItem<TData> = { ...data, id } as BaseStreamItem<TData>
    await this.client.set(key, JSON.stringify(item))
    
    await this.send({ groupId, id }, { type: 'update', data: item })
    
    return item
  }

  async delete(groupId: string, id: string): Promise<BaseStreamItem<TData> | null> {
    const item = await this.get(groupId, id)
    if (item) {
      const key = this.makeKey(groupId, id)
      await this.client.del(key)
      await this.send({ groupId, id }, { type: 'delete', data: item })
    }
    return item
  }

  async getGroup(groupId: string): Promise<BaseStreamItem<TData>[]> {
    const pattern = `${this.makeKey(groupId)}:*`
    const keys = await this.client.keys(pattern)
    
    if (keys.length === 0) return []
    
    const values = await this.client.mget(...keys)
    return values
      .filter((v): v is string => v !== null)
      .map(v => JSON.parse(v))
  }

  async send<T>(
    channel: StateStreamEventChannel,
    event: StateStreamEvent<T>
  ): Promise<void> {
    const channelKey = channel.id 
      ? `motia:stream:events:${channel.groupId}:${channel.id}`
      : `motia:stream:events:${channel.groupId}`
    
    await this.client.publish(channelKey, JSON.stringify(event))
  }

  async subscribe<T>(
    channel: StateStreamEventChannel,
    handler: (event: StateStreamEvent<T>) => void | Promise<void>
  ): Promise<void> {
    const channelKey = channel.id 
      ? `motia:stream:events:${channel.groupId}:${channel.id}`
      : `motia:stream:events:${channel.groupId}`
    
    const subscriber = this.client.duplicate()
    await subscriber.subscribe(channelKey)
    
    subscriber.on('message', async (ch, message) => {
      if (ch === channelKey) {
        const event = JSON.parse(message) as StateStreamEvent<T>
        await handler(event)
      }
    })
  }

  async unsubscribe(channel: StateStreamEventChannel): Promise<void> {
    const channelKey = channel.id 
      ? `motia:stream:events:${channel.groupId}:${channel.id}`
      : `motia:stream:events:${channel.groupId}`
    
    await this.client.unsubscribe(channelKey)
  }

  async clear(groupId: string): Promise<void> {
    const pattern = `${this.makeKey(groupId)}:*`
    const keys = await this.client.keys(pattern)
    
    if (keys.length > 0) {
      await this.client.del(...keys)
    }
  }

  async query(
    groupId: string,
    filter: StreamQueryFilter<TData>
  ): Promise<BaseStreamItem<TData>[]> {
    let items = await this.getGroup(groupId)
    
    if (filter.where) {
      items = items.filter(item => {
        return Object.entries(filter.where!).every(([key, value]) => {
          return (item as any)[key] === value
        })
      })
    }
    
    if (filter.orderBy) {
      items.sort((a, b) => {
        const aVal = (a as any)[filter.orderBy!]
        const bVal = (b as any)[filter.orderBy!]
        const comparison = aVal < bVal ? -1 : aVal > bVal ? 1 : 0
        return filter.orderDirection === 'desc' ? -comparison : comparison
      })
    }
    
    if (filter.offset) {
      items = items.slice(filter.offset)
    }
    
    if (filter.limit) {
      items = items.slice(0, filter.limit)
    }
    
    return items
  }
}
```

### Example 4: Kafka Event Adapter

```typescript
import { Kafka, type Producer, type Consumer, type EachMessagePayload } from 'kafkajs'
import type { EventAdapter, Event, SubscribeOptions, SubscriptionHandle } from '@motiadev/core'
import { v4 as uuidv4 } from 'uuid'

export interface KafkaEventAdapterConfig {
  clientId: string
  brokers: string[]
  groupId?: string
}

export class KafkaEventAdapter implements EventAdapter {
  private kafka: Kafka
  private producer?: Producer
  private consumers: Map<string, Consumer> = new Map()
  private subscriptions: Map<string, SubscriptionHandle> = new Map()
  private groupId: string

  constructor(config: KafkaEventAdapterConfig) {
    this.kafka = new Kafka({
      clientId: config.clientId,
      brokers: config.brokers,
    })
    this.groupId = config.groupId || 'motia-default'
  }

  private async ensureProducer(): Promise<Producer> {
    if (!this.producer) {
      this.producer = this.kafka.producer()
      await this.producer.connect()
    }
    return this.producer
  }

  async emit<TData>(event: Event<TData>): Promise<void> {
    const producer = await this.ensureProducer()
    
    await producer.send({
      topic: event.topic,
      messages: [
        {
          key: event.traceId,
          value: JSON.stringify({
            topic: event.topic,
            data: event.data,
            traceId: event.traceId,
            timestamp: Date.now(),
          }),
        },
      ],
    })
  }

  async subscribe<TData>(
    topic: string,
    handler: (event: Event<TData>) => void | Promise<void>,
    options?: SubscribeOptions
  ): Promise<SubscriptionHandle> {
    const groupId = options?.queue || this.groupId
    const consumerId = uuidv4()
    
    const consumer = this.kafka.consumer({ 
      groupId,
      sessionTimeout: 30000,
    })
    
    await consumer.connect()
    await consumer.subscribe({ topic, fromBeginning: false })
    
    await consumer.run({
      eachMessage: async (payload: EachMessagePayload) => {
        try {
          const content = JSON.parse(payload.message.value?.toString() || '{}')
          await handler(content as Event<TData>)
        } catch (error) {
          console.error('Error processing Kafka message:', error)
        }
      },
    })
    
    this.consumers.set(consumerId, consumer)
    
    const handle: SubscriptionHandle = {
      topic,
      id: consumerId,
      unsubscribe: async () => {
        await this.unsubscribe(handle)
      },
    }
    
    this.subscriptions.set(handle.id, handle)
    return handle
  }

  async unsubscribe(handle: SubscriptionHandle): Promise<void> {
    const consumer = this.consumers.get(handle.id)
    if (consumer) {
      await consumer.disconnect()
      this.consumers.delete(handle.id)
    }
    this.subscriptions.delete(handle.id)
  }

  async shutdown(): Promise<void> {
    if (this.producer) {
      await this.producer.disconnect()
    }
    
    for (const consumer of this.consumers.values()) {
      await consumer.disconnect()
    }
    
    this.consumers.clear()
    this.subscriptions.clear()
  }

  async getSubscriptionCount(topic: string): Promise<number> {
    return Array.from(this.subscriptions.values())
      .filter(sub => sub.topic === topic)
      .length
  }

  async listTopics(): Promise<string[]> {
    return Array.from(new Set(
      Array.from(this.subscriptions.values()).map(sub => sub.topic)
    ))
  }
}
```

### Example 5: Redis Cron Adapter

```typescript
import Redis from 'ioredis'
import type { CronAdapter, CronLock, CronLockInfo } from '@motiadev/core'
import { v4 as uuidv4 } from 'uuid'

export interface RedisCronAdapterConfig {
  host: string
  port: number
  password?: string
  db?: number
  keyPrefix?: string
  lockTTL?: number
  lockRetryDelay?: number
  lockRetryAttempts?: number
  instanceId?: string
  enableHealthCheck?: boolean
}

export class RedisCronAdapter implements CronAdapter {
  private client: Redis
  private keyPrefix: string
  private lockTTL: number
  private lockRetryDelay: number
  private lockRetryAttempts: number
  private instanceId: string
  private enableHealthCheck: boolean

  constructor(config: RedisCronAdapterConfig) {
    this.client = new Redis({
      host: config.host,
      port: config.port,
      password: config.password,
      db: config.db || 0,
    })
    this.keyPrefix = config.keyPrefix || 'motia:cron:lock:'
    this.lockTTL = config.lockTTL || 300000
    this.lockRetryDelay = config.lockRetryDelay || 1000
    this.lockRetryAttempts = config.lockRetryAttempts || 0
    this.instanceId = config.instanceId || `motia-${uuidv4()}`
    this.enableHealthCheck = config.enableHealthCheck ?? true
  }

  private makeKey(jobName: string): string {
    return `${this.keyPrefix}${jobName}`
  }

  async acquireLock(jobName: string, ttl?: number): Promise<CronLock | null> {
    const lockTTL = ttl || this.lockTTL
    const lockId = uuidv4()
    const key = this.makeKey(jobName)
    const now = Date.now()
    const expiresAt = now + lockTTL

    const lock: CronLock = {
      jobName,
      lockId,
      acquiredAt: now,
      expiresAt,
      instanceId: this.instanceId,
    }

    const lockData = JSON.stringify(lock)
    const ttlSeconds = Math.ceil(lockTTL / 1000)

    const result = await this.client.set(
      key,
      lockData,
      'PX',
      lockTTL,
      'NX'
    )

    if (result === 'OK') {
      return lock
    }

    if (this.lockRetryAttempts > 0) {
      for (let attempt = 0; attempt < this.lockRetryAttempts; attempt++) {
        await new Promise(resolve => setTimeout(resolve, this.lockRetryDelay))
        
        const retryResult = await this.client.set(
          key,
          lockData,
          'PX',
          lockTTL,
          'NX'
        )

        if (retryResult === 'OK') {
          return lock
        }
      }
    }

    return null
  }

  async releaseLock(lock: CronLock): Promise<void> {
    const key = this.makeKey(lock.jobName)
    const currentLockData = await this.client.get(key)

    if (!currentLockData) {
      return
    }

    try {
      const currentLock: CronLock = JSON.parse(currentLockData)
      
      if (currentLock.lockId === lock.lockId && currentLock.instanceId === this.instanceId) {
        await this.client.del(key)
      }
    } catch (error) {
      console.error('Error releasing lock:', error)
    }
  }

  async renewLock(lock: CronLock, ttl: number): Promise<boolean> {
    const key = this.makeKey(lock.jobName)
    const currentLockData = await this.client.get(key)

    if (!currentLockData) {
      return false
    }

    try {
      const currentLock: CronLock = JSON.parse(currentLockData)
      
      if (currentLock.lockId === lock.lockId && currentLock.instanceId === this.instanceId) {
        const now = Date.now()
        const renewedLock: CronLock = {
          ...lock,
          expiresAt: now + ttl,
        }

        const result = await this.client.set(
          key,
          JSON.stringify(renewedLock),
          'PX',
          ttl,
          'XX'
        )

        return result === 'OK'
      }
    } catch (error) {
      console.error('Error renewing lock:', error)
    }

    return false
  }

  async isHealthy(): Promise<boolean> {
    if (!this.enableHealthCheck) {
      return true
    }

    try {
      const result = await this.client.ping()
      return result === 'PONG'
    } catch (error) {
      return false
    }
  }

  async shutdown(): Promise<void> {
    const pattern = `${this.keyPrefix}*`
    const keys = await this.client.keys(pattern)

    for (const key of keys) {
      const lockData = await this.client.get(key)
      if (lockData) {
        try {
          const lock: CronLock = JSON.parse(lockData)
          if (lock.instanceId === this.instanceId) {
            await this.client.del(key)
          }
        } catch (error) {
          console.error('Error cleaning up lock during shutdown:', error)
        }
      }
    }

    await this.client.quit()
  }

  async getActiveLocks(): Promise<CronLockInfo[]> {
    const pattern = `${this.keyPrefix}*`
    const keys = await this.client.keys(pattern)
    const locks: CronLockInfo[] = []

    for (const key of keys) {
      const lockData = await this.client.get(key)
      if (lockData) {
        try {
          const lock: CronLock = JSON.parse(lockData)
          locks.push({
            jobName: lock.jobName,
            instanceId: lock.instanceId,
            acquiredAt: lock.acquiredAt,
            expiresAt: lock.expiresAt,
          })
        } catch (error) {
          console.error('Error parsing lock data:', error)
        }
      }
    }

    return locks
  }
}
```

### Example 6: PostgreSQL Cron Adapter

For teams that prefer not to add Redis as a dependency, a database-based cron adapter can be implemented using PostgreSQL's advisory locks or a simple locks table:

```typescript
import { Pool, type PoolClient } from 'pg'
import type { CronAdapter, CronLock, CronLockInfo } from '@motiadev/core'
import { v4 as uuidv4 } from 'uuid'

export interface PostgresCronAdapterConfig {
  connectionString: string
  lockTTL?: number
  instanceId?: string
  tableName?: string
}

export class PostgresCronAdapter implements CronAdapter {
  private pool: Pool
  private lockTTL: number
  private instanceId: string
  private tableName: string

  constructor(config: PostgresCronAdapterConfig) {
    this.pool = new Pool({
      connectionString: config.connectionString,
    })
    this.lockTTL = config.lockTTL || 300000
    this.instanceId = config.instanceId || `motia-${uuidv4()}`
    this.tableName = config.tableName || 'motia_cron_locks'
    
    this.initializeTable()
  }

  private async initializeTable(): Promise<void> {
    const client = await this.pool.connect()
    try {
      await client.query(`
        CREATE TABLE IF NOT EXISTS ${this.tableName} (
          job_name VARCHAR(255) PRIMARY KEY,
          lock_id VARCHAR(255) NOT NULL,
          instance_id VARCHAR(255) NOT NULL,
          acquired_at BIGINT NOT NULL,
          expires_at BIGINT NOT NULL
        )
      `)
      
      await client.query(`
        CREATE INDEX IF NOT EXISTS idx_expires_at 
        ON ${this.tableName}(expires_at)
      `)
    } finally {
      client.release()
    }
  }

  async acquireLock(jobName: string, ttl?: number): Promise<CronLock | null> {
    const lockTTL = ttl || this.lockTTL
    const lockId = uuidv4()
    const now = Date.now()
    const expiresAt = now + lockTTL

    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      
      await client.query(`
        DELETE FROM ${this.tableName}
        WHERE expires_at < $1
      `, [now])

      const result = await client.query(`
        INSERT INTO ${this.tableName} 
        (job_name, lock_id, instance_id, acquired_at, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (job_name) DO NOTHING
        RETURNING *
      `, [jobName, lockId, this.instanceId, now, expiresAt])

      await client.query('COMMIT')

      if (result.rows.length > 0) {
        return {
          jobName,
          lockId,
          acquiredAt: now,
          expiresAt,
          instanceId: this.instanceId,
        }
      }

      return null
    } catch (error) {
      await client.query('ROLLBACK')
      throw error
    } finally {
      client.release()
    }
  }

  async releaseLock(lock: CronLock): Promise<void> {
    await this.pool.query(`
      DELETE FROM ${this.tableName}
      WHERE job_name = $1 
        AND lock_id = $2 
        AND instance_id = $3
    `, [lock.jobName, lock.lockId, this.instanceId])
  }

  async renewLock(lock: CronLock, ttl: number): Promise<boolean> {
    const now = Date.now()
    const expiresAt = now + ttl

    const result = await this.pool.query(`
      UPDATE ${this.tableName}
      SET expires_at = $1
      WHERE job_name = $2 
        AND lock_id = $3 
        AND instance_id = $4
      RETURNING *
    `, [expiresAt, lock.jobName, lock.lockId, this.instanceId])

    return result.rows.length > 0
  }

  async isHealthy(): Promise<boolean> {
    try {
      await this.pool.query('SELECT 1')
      return true
    } catch (error) {
      return false
    }
  }

  async shutdown(): Promise<void> {
    await this.pool.query(`
      DELETE FROM ${this.tableName}
      WHERE instance_id = $1
    `, [this.instanceId])
    
    await this.pool.end()
  }

  async getActiveLocks(): Promise<CronLockInfo[]> {
    const now = Date.now()
    const result = await this.pool.query(`
      SELECT job_name, instance_id, acquired_at, expires_at
      FROM ${this.tableName}
      WHERE expires_at > $1
      ORDER BY acquired_at DESC
    `, [now])

    return result.rows.map(row => ({
      jobName: row.job_name,
      instanceId: row.instance_id,
      acquiredAt: parseInt(row.acquired_at),
      expiresAt: parseInt(row.expires_at),
    }))
  }
}
```

## Integration Points

### 1. Core Server Initialization

Update `packages/core/src/server.ts` to support adapter configuration:

```typescript
export const createServer = (
  lockedData: LockedData,
  eventManager: EventManager | EventAdapter,
  state: StateAdapter,
  config: MotiaServerConfig,
  streamAdapterFactory?: () => StreamAdapter<any>,
  cronAdapter?: CronAdapter
): MotiaServer => {
}
```

### 2. Cron Handler Integration

Update `packages/core/src/cron-handler.ts` to use the cron adapter:

```typescript
import * as cron from 'node-cron'
import { callStepFile } from './call-step-file'
import { generateTraceId } from './generate-trace-id'
import { globalLogger } from './logger'
import type { Motia } from './motia'
import type { CronConfig, Step, CronAdapter } from './types'

export type CronManager = {
  createCronJob: (step: Step<CronConfig>) => void
  removeCronJob: (step: Step<CronConfig>) => void
  close: () => void
}

export const setupCronHandlers = (motia: Motia, cronAdapter?: CronAdapter) => {
  const cronJobs = new Map<string, cron.ScheduledTask>()

  const createCronJob = (step: Step<CronConfig>) => {
    const { config, filePath } = step
    const { cron: cronExpression, name: stepName, flows } = config

    if (!cron.validate(cronExpression)) {
      globalLogger.error('[cron handler] invalid cron expression', {
        expression: cronExpression,
        step: stepName,
      })
      return
    }

    globalLogger.debug('[cron handler] setting up cron job', {
      filePath,
      step: stepName,
      cron: cronExpression,
    })

    const task = cron.schedule(cronExpression, async () => {
      let lock: CronLock | null = null

      if (cronAdapter) {
        try {
          lock = await cronAdapter.acquireLock(stepName, 300000)
          
          if (!lock) {
            globalLogger.debug('[cron handler] failed to acquire lock, skipping execution', {
              step: stepName,
            })
            return
          }

          globalLogger.debug('[cron handler] acquired lock for cron job', {
            step: stepName,
            lockId: lock.lockId,
            instanceId: lock.instanceId,
          })
        } catch (error: any) {
          globalLogger.error('[cron handler] error acquiring lock', {
            error: error.message,
            step: stepName,
          })
          return
        }
      }

      const traceId = generateTraceId()
      const logger = motia.loggerFactory.create({ traceId, flows, stepName })
      const tracer = await motia.tracerFactory.createTracer(traceId, step, logger)

      try {
        await callStepFile({ contextInFirstArg: true, step, traceId, tracer, logger }, motia)
      } catch (error: any) {
        logger.error('[cron handler] error executing cron job', {
          error: error.message,
          step: step.config.name,
        })
      } finally {
        if (lock && cronAdapter) {
          try {
            await cronAdapter.releaseLock(lock)
            globalLogger.debug('[cron handler] released lock for cron job', {
              step: stepName,
              lockId: lock.lockId,
            })
          } catch (error: any) {
            globalLogger.error('[cron handler] error releasing lock', {
              error: error.message,
              step: stepName,
            })
          }
        }
      }
    })

    cronJobs.set(step.filePath, task)
  }

  const removeCronJob = (step: Step<CronConfig>) => {
    const task = cronJobs.get(step.filePath)

    if (task) {
      task.stop()
      cronJobs.delete(step.filePath)
    }
  }

  const close = async () => {
    cronJobs.forEach((task) => task.stop())
    cronJobs.clear()
    
    if (cronAdapter) {
      await cronAdapter.shutdown()
    }
  }

  motia.lockedData.cronSteps().forEach(createCronJob)

  return { createCronJob, removeCronJob, close }
}
```

### 3. Snap CLI Integration

Update `packages/snap/src/dev.ts` and `packages/snap/src/start.ts`:

```typescript
const appConfig: Config = await loadMotiaConfig(baseDir)

const state = appConfig.adapters?.state || createStateAdapter({
  adapter: 'default',
  filePath: path.join(baseDir, motiaFileStoragePath),
})

const eventManager = appConfig.adapters?.events || createEventManager()

const streamAdapterFactory = appConfig.adapters?.streams 
  ? () => appConfig.adapters!.streams!
  : undefined

const cronAdapter = appConfig.adapters?.cron
```

### 3. Docker Support

Update Docker image generation to support external services:

```dockerfile
ENV STATE_ADAPTER=redis
ENV STREAM_ADAPTER=redis
ENV EVENT_ADAPTER=rabbitmq

ENV REDIS_HOST=redis
ENV REDIS_PORT=6379

ENV RABBITMQ_URL=amqp://rabbitmq:5672
```

## Technical Considerations

### Performance Impact

- **State Operations**: Redis adds ~1-2ms latency vs in-memory (acceptable for most use cases)
- **Event Delivery**: RabbitMQ adds ~5-10ms vs in-process (provides reliability guarantees)
- **Stream Updates**: Real-time updates may have slight delay (~10-50ms) but gain horizontal scaling
- **Cron Lock Acquisition**: Redis lock acquisition adds ~2-5ms overhead per cron trigger (negligible for scheduled tasks)

### Scalability Considerations

- **State**: Redis can handle millions of operations per second with clustering
- **Events**: RabbitMQ/Kafka can handle thousands of messages per second per topic
- **Streams**: Redis Streams can handle high throughput with consumer groups
- **Cron Coordination**: Distributed locking scales linearly with number of unique cron jobs, not number of instances

### Cron-Specific Considerations

#### Lock TTL Selection

The lock TTL should be set based on the expected maximum execution time of your cron jobs:
- **Short Jobs** (< 1 minute): Use 60000ms (1 minute) TTL
- **Medium Jobs** (1-5 minutes): Use 300000ms (5 minutes) TTL
- **Long Jobs** (> 5 minutes): Use 600000ms+ (10+ minutes) TTL or implement lock renewal

#### Lock Renewal for Long-Running Jobs

For cron jobs that may take longer than the lock TTL, implement lock renewal:

```typescript
export const handler: Handlers['LongRunningCronJob'] = async ({ logger }) => {
  const renewInterval = setInterval(async () => {
    const renewed = await cronAdapter.renewLock(currentLock, 300000)
    if (!renewed) {
      logger.warn('Failed to renew lock, job may be interrupted')
    }
  }, 240000)

  try {
    logger.info('Starting long-running job')
  } finally {
    clearInterval(renewInterval)
  }
}
```

#### Monitoring Cron Execution

To monitor which instances are executing cron jobs:

```typescript
const activeLocks = await cronAdapter.getActiveLocks()
console.log('Active cron jobs:', activeLocks)
```

#### Handling Missed Executions

If all instances fail to acquire a lock (e.g., due to network issues), the cron job will be skipped for that execution. Consider:
- Setting up alerts for missed executions
- Using retry mechanisms in the cron adapter configuration
- Implementing idempotent cron handlers that can safely re-execute

#### Development vs Production

For development environments where horizontal scaling is not needed:
- Omit the `cron` adapter from configuration
- Cron jobs will execute normally without distributed locking
- This reduces external dependencies during development

For production:
- Always configure a cron adapter when running multiple instances
- Use managed services (AWS ElastiCache, Amazon RDS) for high availability
- Set appropriate lock TTLs based on job execution times

### Compatibility and Migration

- **Backward Compatibility**: File and memory adapters remain the default
- **No Breaking Changes**: Existing applications work without modifications
- **Migration Path**: 
  1. Add adapter configuration to `motia.config.ts`
  2. Deploy with external services
  3. Scale to multiple instances
- **Data Migration**: Provide utility to copy state from file to Redis

### Risk Assessment

- **External Service Dependency**: Applications become dependent on Redis/RabbitMQ availability
  - **Mitigation**: Use managed services (AWS ElastiCache, Amazon MQ) with high availability
- **Configuration Complexity**: More configuration options may confuse users
  - **Mitigation**: Provide clear documentation and starter templates
- **Cost**: External services cost money
  - **Mitigation**: Keep file/memory adapters as free defaults for development

## Alternatives Considered

### Alternative 1: Built-in Distributed State

- **Pros**: No external dependencies, simpler deployment
- **Cons**: Reinventing the wheel, complex to implement correctly, less battle-tested
- **Decision**: Rejected - better to use proven distributed systems

### Alternative 2: Single Adapter for All Components

- **Pros**: Simpler configuration, fewer connections
- **Cons**: Less flexibility, forces suboptimal choices (Redis for everything)
- **Decision**: Rejected - different components have different needs

### Alternative 3: Automatic Clustering

- **Pros**: No configuration needed, works out of the box
- **Cons**: Complex to implement, less control, vendor lock-in
- **Decision**: Rejected - explicit configuration is more transparent

## Testing Strategy

### Unit Testing

- Test each adapter implementation in isolation
- Mock Redis/RabbitMQ/Kafka clients
- Verify correct method implementations
- Test error handling and edge cases

### Integration Testing

- Use Testcontainers to spin up real Redis/RabbitMQ/Kafka instances
- Test end-to-end flows with multiple Motia instances
- Verify data consistency across instances
- Test failover scenarios

### User Acceptance Testing

- Provide example applications using each adapter
- Document common deployment patterns
- Create migration guides for existing applications

## Success Metrics

### Technical Success

- **Zero Data Loss**: All state operations are atomic and durable
- **Horizontal Scaling**: Applications can scale to 10+ instances
- **Performance**: < 5ms p99 latency overhead vs in-memory
- **Reliability**: 99.9% uptime with managed services

### User Success

- **Adoption**: 30% of production deployments use adapters within 6 months
- **Documentation Quality**: < 5% support requests about adapter configuration
- **Community**: 3+ community-contributed adapters within first year

## Future Considerations

- **Adapter Marketplace**: Registry of community adapters
- **Adapter Testing Framework**: Standardized test suite for custom adapters
- **Multi-Cloud Support**: Adapters for AWS, GCP, Azure native services
- **Adapter Monitoring**: Built-in metrics and health checks
- **Adapter Migration Tools**: Utilities to migrate between adapters
- **Hybrid Adapters**: Combine multiple backends (Redis + DynamoDB)
- **Adapter Fallback**: Automatic fallback to file adapter on external service failure

## Package Structure

Adapters will be published as separate packages:

```
@motiadev/adapter-redis-state
@motiadev/adapter-redis-streams  
@motiadev/adapter-redis-cron
@motiadev/adapter-rabbitmq-events
@motiadev/adapter-kafka-events
@motiadev/adapter-aws-dynamodb-state
@motiadev/adapter-aws-sqs-events
@motiadev/adapter-postgres-cron
@motiadev/adapter-mysql-cron
```

## Implementation Status

### Completed (2025-10-20)

All core adapter interfaces and integration points have been implemented:

#### 1. Adapter Interfaces Created
- ✅ **EventAdapter** (`packages/core/src/adapters/event-adapter.ts`)
  - Full interface with emit, subscribe, unsubscribe, shutdown methods
  - SubscribeOptions and SubscriptionHandle types
  - DefaultQueueEventAdapter wrapping existing QueueManager as local adapter implementation

- ✅ **CronAdapter** (`packages/core/src/adapters/cron-adapter.ts`)
  - Complete interface with distributed locking support
  - CronLock, CronLockInfo, and CronAdapterConfig types
  - Methods: acquireLock, releaseLock, renewLock, isHealthy, shutdown, getActiveLocks

- ✅ **StreamAdapter** Enhanced (`packages/core/src/streams/adapters/stream-adapter.ts`)
  - Added StreamQueryFilter interface
  - Added subscribe, unsubscribe, clear, and query methods
  - Maintains backward compatibility

- ✅ **Default Adapter Implementations**
  - DefaultQueueEventAdapter (`packages/core/src/adapters/`) - Wraps QueueManager for local event handling
  - FileStateAdapter and MemoryStateAdapter (`packages/core/src/state/adapters/`) - Local state implementations
  - FileStreamAdapter and MemoryStreamAdapter (`packages/core/src/streams/adapters/`) - Local stream implementations
  - QueueManager treated as local adapter implementation

#### 2. Core Integration
- ✅ **Config Type** (`packages/core/src/types/app-config-types.ts`)
  - Added AdapterConfig type with optional state, streams, events, cron fields
  - Extended Config type to include optional adapters field
  - Fully backward compatible

- ✅ **Event Manager** (`packages/core/src/event-manager.ts`)
  - Refactored to use EventAdapter pattern exclusively
  - Creates DefaultQueueEventAdapter when no custom adapter provided
  - QueueManager is now treated as a local adapter implementation
  - Cleaner architecture with consistent adapter interface

- ✅ **Cron Handler** (`packages/core/src/cron-handler.ts`)
  - Integrated CronAdapter for distributed locking
  - Implements lock acquisition before job execution
  - Prevents duplicate executions across instances
  - Changed close method to async for adapter shutdown

- ✅ **Server Initialization** (`packages/core/src/server.ts`)
  - Added AdapterOptions type
  - Updated createServer to accept optional adapters parameter
  - Integrated adapters into cron handler and event manager
  - Added adapter shutdown in close function

#### 3. CLI Integration
- ✅ **Config Loader** (`packages/snap/src/load-motia-config.ts`)
  - Loads motia.config.ts from project directory
  - Graceful error handling

- ✅ **Dev Server** (`packages/snap/src/dev.ts`)
  - Loads and applies adapter configuration
  - Falls back to defaults when not configured

- ✅ **Start Server** (`packages/snap/src/start.ts`)
  - Loads and applies adapter configuration
  - Falls back to defaults when not configured

#### 4. Type Exports
- ✅ **Core Exports** (`packages/core/index.ts`)
  - Exported all adapter interfaces and types
  - Exported AdapterConfig and Config types
  - Full TypeScript support

### Implementation Files

```
packages/core/src/
├── adapters/
│   ├── event-adapter.ts                    # EventAdapter interface
│   ├── cron-adapter.ts                     # CronAdapter interface
│   └── default-queue-event-adapter.ts      # Local QueueManager adapter
├── types/
│   └── app-config-types.ts                 # Config with adapters field
├── state/
│   ├── state-adapter.ts                    # StateAdapter interface
│   ├── create-state-adapter.ts             # State adapter factory
│   └── adapters/
│       ├── default-state-adapter.ts        # FileStateAdapter implementation
│       └── memory-state-adapter.ts         # MemoryStateAdapter implementation
├── streams/
│   └── adapters/
│       ├── stream-adapter.ts               # Enhanced StreamAdapter interface
│       ├── file-stream-adapter.ts          # FileStreamAdapter implementation
│       └── memory-stream-adapter.ts        # MemoryStreamAdapter implementation
├── queue-manager.ts                        # In-memory queue (used by local adapter)
├── event-manager.ts                        # Refactored to use adapters
├── cron-handler.ts                         # Integrated CronAdapter
└── server.ts                               # Updated with adapter support

packages/snap/src/
├── load-motia-config.ts                    # Config loader utility
├── dev.ts                                  # Updated dev server
└── start.ts                                # Updated start server
```

### Pending Implementation

The following items are ready for community contribution or future implementation:

#### Adapter Packages (Not Yet Implemented)
These adapter implementations follow the interfaces defined in the core:

- ⏳ `@motiadev/adapter-redis-state` - Redis-based state management
- ⏳ `@motiadev/adapter-redis-streams` - Redis Streams for real-time data
- ⏳ `@motiadev/adapter-redis-cron` - Redis-based distributed cron locking
- ⏳ `@motiadev/adapter-rabbitmq-events` - RabbitMQ event distribution
- ⏳ `@motiadev/adapter-kafka-events` - Kafka event streaming
- ⏳ `@motiadev/adapter-postgres-cron` - PostgreSQL-based cron locking
- ⏳ `@motiadev/adapter-aws-dynamodb-state` - DynamoDB state storage
- ⏳ `@motiadev/adapter-aws-sqs-events` - AWS SQS event queue

Example implementations are provided in this RFC and can be used as templates for creating these packages.

#### Documentation (Pending)
- ⏳ Adapter configuration guide
- ⏳ Migration guide for existing applications
- ⏳ Individual adapter package documentation
- ⏳ Best practices for horizontal scaling
- ⏳ Troubleshooting guide

#### Testing (Pending)
- ⏳ Unit tests for adapter interfaces
- ⏳ Integration tests with real adapters
- ⏳ Multi-instance scenario tests
- ⏳ Backward compatibility tests

#### Examples (Pending)
- ⏳ Redis-based horizontal scaling example
- ⏳ RabbitMQ event distribution example
- ⏳ Cron coordination example
- ⏳ Full production deployment example

### Usage Example

With the implementation complete, users can now configure adapters in their `motia.config.ts`:

```typescript
import { config } from '@motiadev/core'
import { RedisStateAdapter } from '@motiadev/adapter-redis-state'
import { RedisCronAdapter } from '@motiadev/adapter-redis-cron'
import { RabbitMQEventAdapter } from '@motiadev/adapter-rabbitmq-events'

export default config({
  adapters: {
    state: new RedisStateAdapter({
      host: process.env.REDIS_HOST || 'localhost',
      port: parseInt(process.env.REDIS_PORT || '6379'),
      password: process.env.REDIS_PASSWORD,
    }),
    
    events: new RabbitMQEventAdapter({
      url: process.env.RABBITMQ_URL || 'amqp://localhost',
      exchangeName: 'motia.events',
      exchangeType: 'topic',
    }),
    
    cron: new RedisCronAdapter({
      host: process.env.REDIS_HOST || 'localhost',
      port: parseInt(process.env.REDIS_PORT || '6379'),
      lockTTL: 300000,
    }),
  },
})
```

### Backward Compatibility

✅ **Fully Maintained**: All changes are optional and existing applications work without modifications:
- Adapters are optional in configuration
- Default file/memory adapters used when not configured
- All new parameters are optional
- No breaking changes to existing APIs

### Next Steps for Community

1. **Implement Adapter Packages**: Use the example implementations in this RFC to create adapter packages
2. **Write Documentation**: Create guides for adapter configuration and migration
3. **Add Tests**: Implement comprehensive testing for adapters
4. **Create Examples**: Build example applications demonstrating horizontal scaling
5. **Share Adapters**: Contribute custom adapters to the community

## Conclusion

This RFC has been successfully implemented, providing Motia with a comprehensive adapter pattern that enables horizontal scaling while maintaining backward compatibility. The core infrastructure is now in place for:

- **Pluggable State Management**: Custom adapters for any distributed storage system
- **Distributed Event Handling**: Scalable pub/sub across multiple instances
- **Coordinated Cron Execution**: Prevents duplicate job executions through distributed locking
- **Flexible Stream Processing**: Real-time data updates across instances

The implementation leverages battle-tested distributed systems like Redis, RabbitMQ, and Kafka through clean, well-defined interfaces. The pluggable adapter system allows users to choose the best technology for their specific needs, whether that's cost, performance, or operational preferences.

**Cron-Steps in Horizontal Scaling**: The CronAdapter implementation solves the duplicate execution problem inherent in distributed cron scheduling. By using distributed locking mechanisms, scheduled tasks execute exactly once across all instances, preventing data inconsistencies and wasted resources.

This change makes Motia viable for production environments and unlocks deployment to Kubernetes, AWS ECS, and other container orchestration platforms that require horizontal scaling for high availability and performance. With proper cron coordination, teams can confidently scale their Motia applications knowing that scheduled tasks will execute reliably and without duplication.

The foundation is complete—the community can now build upon these interfaces to create adapter implementations for their preferred infrastructure.

