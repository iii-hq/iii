---
name: iii-trigger-schemas
description: >-
  Canonical built-in iii trigger config and call payload shapes. Use when
  generating or validating HTTP, cron, queue, pubsub, state, stream, or log
  trigger registrations and handler input types.
---

# Trigger Schemas

Use this skill when an agent needs the exact config shape for built-in trigger registrations and the payload shape delivered to handlers.

## HTTP Trigger

Registration config:

```json
{
  "api_path": "/orders/:id",
  "http_method": "POST",
  "condition_function_id": "optional::condition"
}
```

Handler payload:

```json
{
  "query_params": {},
  "path_params": {},
  "headers": {},
  "path": "/orders/123",
  "method": "POST",
  "body": {}
}
```

`api_path` must use a leading slash.

## Cron Trigger

Registration config:

```json
{
  "expression": "0 0 9 * * * *",
  "condition_function_id": "optional::condition"
}
```

Handler payload:

```json
{
  "trigger": "cron",
  "job_id": "job-id",
  "scheduled_time": "2026-05-19T09:00:00Z",
  "actual_time": "2026-05-19T09:00:00Z"
}
```

Use `expression`, not `cron`.

## Queue Trigger

Registration config:

```json
{
  "topic": "payments",
  "condition_function_id": "optional::condition",
  "queue_config": {}
}
```

Handler payload is the message payload that was enqueued or published for that topic.

## Pub/Sub Subscribe Trigger

Registration config:

```json
{
  "topic": "orders.created",
  "condition_function_id": "optional::condition"
}
```

Handler payload is the event payload published to the topic.

## State Trigger

Registration config:

```json
{
  "scope": "orders",
  "key": "optional-key",
  "condition_function_id": "optional::condition"
}
```

Handler payload:

```json
{
  "type": "state",
  "event_type": "state:updated",
  "scope": "orders",
  "key": "order-123",
  "old_value": {},
  "new_value": {}
}
```

`event_type` can be `state:created`, `state:updated`, or `state:deleted`.

## Stream Trigger

Registration config:

```json
{
  "stream_name": "chat",
  "group_id": "room-1",
  "item_id": "optional-item",
  "condition_function_id": "optional::condition"
}
```

Stream join/leave configs use `stream_name` plus optional `condition_function_id`.

Handler payload contains stream event details such as stream name, group, item, event type, and data. Use `iii-realtime-streams` for worker-backed stream functions, and `iii-channels` for channel refs/binary transport.

## Log Trigger

Registration config:

```json
{
  "level": "warn"
}
```

Handler payload contains OpenTelemetry-style log data: timestamp, severity, body, attributes, trace/span IDs, resource attributes, service metadata, and instrumentation scope fields.

## Pattern Boundaries

- For registering and invoking functions, prefer `iii-functions-and-triggers`.
- For HTTP endpoint behavior, prefer `iii-http-endpoints`.
- For trigger action modes, prefer `iii-trigger-actions`.
- For custom trigger type authoring lifecycle, prefer `iii-custom-triggers`.

## When to Use

- Use this skill when the task asks for built-in trigger config, handler payload shape, validation schemas, or generated function input types for built-in triggers.

## Boundaries

- Use `expression` for cron config, not `cron`.
- Use leading slashes for HTTP `api_path` values.
- Do not generate removed service APIs or adapter-extension APIs.
