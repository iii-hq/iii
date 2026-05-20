---
name: iii-error-handling
description: >-
  Handle iii engine and SDK errors across Node, Python, Rust, and browser
  workers. Use when interpreting error codes, retryability, RBAC denial,
  timeouts, handler failures, or SDK-specific exception surfaces.
---

# Error Handling

iii has two broad error classes: SDK/local errors and engine/remote invocation errors. Agents should branch on the error code instead of matching only message strings.

## Core Error Codes

| Code | Meaning | Typical handling |
| --- | --- | --- |
| `function_not_found` | No registered function is available under that ID | Check function ID, worker install/startup, discovery, and trigger type hints |
| `function_not_invokable` | Registration exists but cannot be invoked as a normal function | Inspect registration/invocation type |
| `invocation_failed` | Handler or HTTP-invoked function failed | Inspect handler logs, stacktrace, and payload validation |
| `invocation_stopped` | Invocation was cancelled or stopped by the engine/runtime | Treat as failed work; decide whether caller should retry |
| `FORBIDDEN` | RBAC denied the action | Do not retry blindly; inspect policy, auth context, and allowed functions |
| `TIMEOUT` | Invocation exceeded configured timeout | Increase timeout only if the workload is expected to run long; otherwise optimize or enqueue |

## Handler vs Engine Errors

- Handler errors originate in user function code or HTTP-invoked endpoints.
- Engine errors originate in routing, invocation state, RBAC, protocol handling, or timeout enforcement.
- Queue retries only apply to enqueued work. Synchronous failures are returned directly to the caller.
- Void dispatch does not return handler results, so use logs/observability for failures.

## Retryability

- Retry transient `TIMEOUT`, transport, or worker reconnect failures only when the operation is idempotent.
- Do not retry `FORBIDDEN` without changing auth/policy.
- Do not retry `function_not_found` by calling the same ID repeatedly; discover functions or install/start the missing worker.
- For reliable background work, use `TriggerAction.Enqueue({ queue })` and queue retry/DLQ policy.

## SDK Surfaces

### Node

```typescript
import { IIIInvocationError } from 'iii-sdk'

try {
  await iii.trigger({ function_id: 'orders::charge', payload })
} catch (error) {
  if (error instanceof IIIInvocationError && error.code === 'FORBIDDEN') {
    throw new Error('Policy denied orders::charge')
  }
  throw error
}
```

### Python

```python
from iii import IIIForbiddenError, IIIInvocationError, IIITimeoutError

try:
    result = iii.trigger({"function_id": "orders::charge", "payload": payload})
except IIIForbiddenError:
    raise RuntimeError("Policy denied orders::charge")
except IIITimeoutError:
    raise RuntimeError("orders::charge timed out")
except IIIInvocationError as exc:
    raise RuntimeError(f"{exc.code}: {exc.message}")
```

### Rust

```rust
match iii.trigger(request).await {
    Ok(value) => value,
    Err(iii_sdk::IIIError::Remote { code, message, .. }) if code == "FORBIDDEN" => {
        return Err(format!("policy denied: {message}").into());
    }
    Err(err) => return Err(err.into()),
}
```

### Browser

Browser trigger calls reject with JavaScript errors. Preserve the engine-provided code/message when present and show policy failures as permission errors in UI.

## Pattern Boundaries

- For RBAC policy design, prefer `iii-worker-rbac`.
- For invocation modes and retries, prefer `iii-trigger-actions`.
- For logs/traces around failures, prefer `iii-observability`.

## When to Use

- Use this skill when the task mentions iii errors, exception handling, failed invocations, timeouts, forbidden calls, retry behavior, or SDK error classes.

## Boundaries

- Do not retry non-idempotent work automatically unless it is enqueued under queue policy.
- Do not treat RBAC denial as a missing worker.
- Do not generate removed service APIs or adapter-extension APIs.
