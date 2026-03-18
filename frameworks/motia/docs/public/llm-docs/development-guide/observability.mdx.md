---
title: Observability
description: See what's happening in your Motia app with logging, tracing, and debugging
---

<video controls className="mb-8 w-full rounded-xl" poster="https://assets.motia.dev/images/gifs/v1/5-motia-logging.gif">
  <source src="https://assets.motia.dev/videos/mp4/site/v1/5-motia-logging.mp4" type="video/mp4" />
</video>

Your app is running. But what's actually happening inside?
- Is that API getting hit?
- Did the message get enqueued?
- Why did that Step fail?
- Which user triggered this flow?

Motia gives you everything you need to answer these questions.

---

## Logging

Every Step can use the `logger` imported from Motia. Use it to see what's happening.

### Log Levels

| Level | When to use it |
| ----- | -------------- |
| `info` | Normal stuff - "User created", "Order processed" |
| `warn` | Something's weird but not broken - "High API usage", "Slow response" |
| `error` | Things broke - Failed API calls, exceptions, crashes |
| `debug` | Deep debugging - Raw data, internal state, timing info |

---

## How to Log

<Tabs items={['TypeScript', 'Python', 'JavaScript']}>
  <Tab value='TypeScript'>
    ```typescript
    import { logger } from 'motia'

    export const handler: Handlers<typeof config> = async (input) => {
      logger.info('Processing order')

      logger.info('Order created', {
        orderId: input.id,
        total: input.total
      })

      try {
        await chargeCard(input.paymentMethod)
      } catch (error) {
        logger.error('Payment failed', {
          error: error.message,
          orderId: input.id
        })
      }

      if (input.total > 1000) {
        logger.warn('Large order', {
          total: input.total,
          threshold: 1000
        })
      }

      logger.debug('Raw input', { input })
    }
    ```
  </Tab>
  <Tab value='Python'>
    ```python
    from motia import logger

    async def handler(input):
        logger.info('Processing order')

        logger.info('Order created', {
            'order_id': input.get("id"),
            'total': input.get("total")
        })

        try:
            await charge_card(input.get("payment_method"))
        except Exception as error:
            logger.error('Payment failed', {
                'error': str(error),
                'order_id': input.get("id")
            })

        if input.get("total", 0) > 1000:
            logger.warn('Large order', {
                'total': input.get("total"),
                'threshold': 1000
            })

        logger.debug('Raw input', {'input': input})
    ```
  </Tab>
  <Tab value='JavaScript'>
    ```javascript
    import { logger } from 'motia'

    export const handler = async (input) => {
      logger.info('Processing order')

      logger.info('Order created', {
        orderId: input.id,
        total: input.total
      })

      try {
        await chargeCard(input.paymentMethod)
      } catch (error) {
        logger.error('Payment failed', {
          error: error.message,
          orderId: input.id
        })
      }

      if (input.total > 1000) {
        logger.warn('Large order', {
          total: input.total,
          threshold: 1000
        })
      }

      logger.debug('Raw input', { input })
    }
    ```
  </Tab>
</Tabs>

Always add context data to your logs. `{ orderId: '123' }` is way more useful than just a message.

---

## Where to See Logs

Start your app:

```bash
npm run dev
```

Logs appear in **two places**:

### 1. Your Terminal

See logs right where you ran `npm run dev`:

```
[INFO] Processing order { orderId: '123', total: 99.99 }
[INFO] Order created { orderId: '123' }
[INFO] Payment successful
```

### 2. The iii Development Console

Open the [iii development console](https://iii.dev/docs) and click on your flow. Logs show up in real-time with:
- Timestamps
- Which Step logged it
- The trace ID (to follow a request through the entire flow)
- Full context data

![Real-time logs in the iii Console](/console/logs-detail.png)

---

## Tracing

Every request gets a unique `traceId`. This lets you follow a single request through your entire flow.

```typescript
import { logger, enqueue } from 'motia'

export const handler: Handlers<typeof config> = async ({ request }, { traceId }) => {
  logger.info('Order started', { traceId })

  await enqueue({
    topic: 'process.payment',
    data: { orderId: '123' }
  })

  return { status: 200, body: { traceId } }
}
```

**In the iii development console:**
- Click any log entry
- See all logs with the same `traceId`
- Follow the request from start to finish

![Traces waterfall view in the iii Console](/console/traces-detail.png)

---

## Debug Mode

Want more detailed logs?

```bash
npm run dev -- --debug
```

This enables `debug` level logs. You'll see everything - raw inputs, internal state, timing info.

**In production:** Don't use debug mode (it's slow and logs everything).

---

## Tips for Better Logs

### Use Objects, Not Strings

**Good:**
```typescript
logger.info('Payment processed', {
  paymentId: '123',
  amount: 100,
  status: 'success'
})
```

**Bad:**
```typescript
logger.info(`Payment 123 processed: amount=100`)
```

Why? Objects are searchable, filterable, and easier to parse.

### Track Performance

```typescript
import { logger } from 'motia'

export const handler: Handlers<typeof config> = async (input) => {
  const start = performance.now()

  await processOrder(input)

  logger.info('Order processed', {
    duration: performance.now() - start,
    orderId: input.id
  })
}
```

### Log Errors Properly

```typescript
try {
  await riskyOperation()
} catch (error) {
  logger.error('Operation failed', {
    error: error.message,
    stack: error.stack,
    input: input.id
  })
  throw error
}
```

---

## Debugging Workflows

**Problem:** Something's not working, but where?

**Steps to debug:**

1. **Check terminal logs** - See which Steps ran
2. **Open the iii development console** at [http://localhost:3113](http://localhost:3113)
3. **Click your flow** - See the visual diagram
4. **Expand logs panel** - See all logs in chronological order
5. **Click a log** - Filter by that `traceId` to follow the request
6. **Check each Step** - See where it failed

![Traces waterfall in the iii Console](/console/traces-waterfall.png)

### Common Issues

**API not responding?**
- Check if the Step ran: Look for logs with your Step's name
- Check the response: Look for `status: 200` in logs

**Message not being processed?**
- Check if `enqueue()` was called: Search logs for "enqueue"
- Check the topic name: Make sure it matches the queue trigger topic

**Step not running?**
- Check if it's discovered: Look for `[CREATED] Step` in startup logs
- Check the file name: Must contain `.step.` or `_step`

---

## Remember

- **Log everything important** - But not everything (no sensitive data!)
- **Use `traceId`** - Follow requests through your entire flow
- **Check the iii development console** - Visual debugging is easier
- **Use objects** - Don't log strings, log objects
- **Debug mode** - Only for development, never in production

---
