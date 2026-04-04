---
title: Project Structure
description: Learn about Motia's project structure, file organization, and automatic step discovery system for building scalable workflow applications.
---

# Project Structure

Understanding how to organize your Motia project is crucial for building maintainable and scalable workflow applications. This guide covers the directory structure, file naming conventions, and Motia's automatic step discovery system.

## Basic Project Structure

Here's what a typical Motia project looks like:

<Folder name="my-motia-project" defaultOpen>
  <Folder name="src" defaultOpen>
    <File name="api-gateway.step.ts" />
    <File name="data-processor_step.py" />
    <File name="send-notification.step.js" />
    <File name="send-notification.tsx" />
  </Folder>
  <File name="package.json" />
  <File name="requirements.txt" />
  <File name="tsconfig.json" />
  <File name="types.d.ts" />
  <File name="config.yaml" />
</Folder>

### File Descriptions

| File | Purpose | Type | Auto-Generated |
|------|---------|------|----------------|
| `api-gateway.step.ts` | TypeScript API endpoint | User Code | - |
| `data-processor_step.py` | Python data processing | User Code | - |
| `send-notification.step.js` | JavaScript automation | User Code | - |
| `send-notification.tsx` | Optional UI override component | User Code | - |
| `package.json` | Node.js dependencies (if using JS/TS) | Config | - |
| `requirements.txt` | Python dependencies (if using Python) | Config | - |
| `tsconfig.json` | TypeScript config (if using TypeScript) | Config | - |
| `types.d.ts` | **Type definitions for your project** | **Generated** | **By TypeScript** |
| `config.yaml` | iii configuration | Config | - |

<Callout type="info">
**Flexible Step Discovery**

Motia automatically discovers and registers steps from your `src/` directory. You're **not required** to use a specific directory structure - organize your code however makes sense for your project!
</Callout>

<Callout type="success">
<strong>Directory flexibility and organization</strong>

- **Recursive discovery** - nest steps in subfolders as deeply as you need (e.g., `src/api/v1/users.step.ts`, `src/services/email/send_step.py`)
- **Organize freely** - structure by feature, language, team, or any pattern that works for you
</Callout>

## Automatic Step Discovery

<Callout type="default">
**Key Concept: Automatic Discovery**

Motia automatically discovers and registers **any file** that follows the `.step.` naming pattern as a workflow step, **regardless of where it's located** in your `src/` directory. No manual registration required - just create a file with the right naming pattern and Motia will find it.
</Callout>

### Discovery Rules

Motia scans your project and automatically registers files as steps based on these simple rules:

1. **File must contain `.step.` or `_step.` in the filename** (e.g., `my-task.step.ts`, `my_task_step.py`)
2. **File must export a `config` object** defining the step configuration
3. **File must export a `handler` function** containing the step logic
4. **File extension determines the runtime** (`.ts` = TypeScript, `.py` = Python, `.js` = JavaScript)

When you start the dev server, Motia will:
- **Recursively scan** the `src/` directory
- **Find all files** matching `*.step.*` or `*_step.*` patterns in the `src/` directory
- Parse their `config` exports to understand step types and connections
- Register them in the workflow engine

<Callout type="success">
**No directory requirement** - Steps are discoverable from anywhere within `src/`, regardless of folder depth or organization pattern.
</Callout>

## File Naming Convention

Motia uses this specific pattern for automatic step discovery:

```
descriptive-name.step.[extension]
```

<Callout type="warning">
The `.step.` part in the filename is **required** - this is how Motia identifies which files are workflow steps during automatic discovery.
</Callout>

### Supported Languages & Extensions

| Language | Extension | Example Step File | Runtime |
|----------|-----------|-------------------|---------|
| **TypeScript** | `.ts` | `user-registration.step.ts` | Node.js with TypeScript |
| **Python** | `.py` | `data-analysis_step.py` | Python interpreter |
| **JavaScript** | `.js` | `send-notification.step.js` | Node.js |

### Naming Examples by Step Type

| Step Type | TypeScript | Python | JavaScript |
|-----------|------------|---------|-----------|
| **API Endpoint** | `auth-api.step.ts` | `auth-api_step.py` or `auth_api_step.py` | `auth-api.step.js` |
| **Queue Handler** | `process-order.step.ts` | `process-order_step.py` or `process_order_step.py` | `process-order.step.js` |
| **Cron Job** | `daily-report.step.ts` | `daily-report_step.py` or `daily_report_step.py` | `daily-report.step.js` |
| **Data Processing** | `transform-data.step.ts` | `ml-analysis_step.py` or `ml_analysis_step.py` | `data-cleanup.step.js` |

## Step Organization Patterns

<Callout type="info">
All examples below use `src/` as the root directory.
</Callout>

<Tabs items={["Sequential", "Feature-Based", "Language-Specific", "Mixed Directories"]}>
<Tab value="Sequential">

### Sequential Flow Organization
Perfect for linear workflows where order matters:

<Folder name="src" defaultOpen>
  <File name="api-start.step.ts" />
  <File name="validate-data_step.py" />
  <File name="process-payment.step.js" />
  <File name="send-confirmation.step.ts" />
  <File name="cleanup_step.py" />
</Folder>

| Step | Language | Purpose |
|------|----------|---------|
| `api-start.step.ts` | TypeScript | API endpoint |
| `validate-data_step.py` | Python | Data validation |
| `process-payment.step.js` | JavaScript | Payment processing |
| `send-confirmation.step.ts` | TypeScript | Email service |
| `cleanup_step.py` | Python | Cleanup tasks |

</Tab>
<Tab value="Feature-Based">

### Feature-Based Organization
Organize by business domains for complex applications:

<Folder name="src" defaultOpen>
  <Folder name="authentication" defaultOpen>
    <File name="login.step.ts" />
    <File name="verify-token_step.py" />
    <File name="logout.step.js" />
  </Folder>
  <Folder name="payment" defaultOpen>
    <File name="process-payment.step.ts" />
    <File name="fraud-detection_step.py" />
    <File name="webhook.step.js" />
  </Folder>
  <Folder name="notification" defaultOpen>
    <File name="email_step.py" />
    <File name="sms.step.js" />
    <File name="push.step.ts" />
  </Folder>
</Folder>

**Benefits:**
- Logical grouping by business domain
- Easy to locate related functionality
- Team ownership by feature area
- Independent scaling and deployment

</Tab>
<Tab value="Language-Specific">

### Language-Specific Organization
Group by programming language for team specialization:

<Folder name="src" defaultOpen>
  <Folder name="typescript" defaultOpen>
    <File name="api-gateway.step.ts" />
    <File name="user-management.step.ts" />
    <File name="data-validation.step.ts" />
  </Folder>
  <Folder name="python" defaultOpen>
    <File name="ml-processing_step.py" />
    <File name="data-analysis_step.py" />
    <File name="image-processing_step.py" />
  </Folder>
  <Folder name="javascript" defaultOpen>
    <File name="automation.step.js" />
    <File name="webhook-handlers.step.js" />
    <File name="integrations.step.js" />
  </Folder>
</Folder>

**Benefits:**
- Team specialization by language
- Consistent tooling and patterns
- Easy onboarding for language experts
- Shared libraries and utilities

</Tab>
<Tab value="Mixed Directories">

### Mixed Directory Organization
Organize by different concerns within `src/`:

<Folder name="project-root" defaultOpen>
  <Folder name="src" defaultOpen>
    <Folder name="api" defaultOpen>
      <File name="users.step.ts" />
      <File name="products.step.ts" />
    </Folder>
    <Folder name="services" defaultOpen>
      <File name="email-service_step.py" />
      <File name="payment-service.step.js" />
    </Folder>
    <Folder name="workflows" defaultOpen>
      <File name="onboarding.step.ts" />
      <File name="analytics_step.py" />
    </Folder>
  </Folder>
</Folder>

**Benefits:**
- Combine organizational patterns
- Separate concerns (e.g., APIs, services, workflows)
- Team autonomy in different parts of the codebase

</Tab>
</Tabs>

## Language-Specific Configuration

### TypeScript/JavaScript Projects

For Node.js-based steps, you'll need:

```json title="package.json"
{
  "name": "my-motia-app",
  "version": "1.0.0",
  "scripts": {
    "dev": "iii",
    "build": "motia build",
    "start": "motia start"
  },
  "dependencies": {
    "motia": "^0.5.12-beta.121",
    "zod": "^3.24.4"
  },
  "devDependencies": {
    "typescript": "^5.7.3",
    "@types/node": "^20.0.0"
  }
}
```

```json title="tsconfig.json (for TypeScript)"
{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Node",
    "esModuleInterop": true,
    "strict": true,
    "skipLibCheck": true
  },
  "include": ["**/*.ts", "**/*.tsx"],
  "exclude": ["node_modules", "dist"]
}
```

### Python Projects

For Python-based steps:

```text title="requirements.txt"
# Core Motia dependency (install via pip install motia-python)
motia>=0.5.12

# Common dependencies
requests>=2.28.0
pydantic>=1.10.0

# Data processing (if needed)
pandas>=1.5.0
numpy>=1.21.0
```

## Step Discovery Examples

Let's see how Motia discovers different step types:

### Example 1: TypeScript API Step

```typescript title="src/user-api.step.ts"
import { type Handlers, type StepConfig, enqueue } from 'motia'

export const config = {
  name: 'user-api',
  description: 'Fetch users and enqueue event',
  triggers: [
    { type: 'http', path: '/users', method: 'GET' },
  ],
  enqueues: ['users.fetched'],
  flows: ['user-management'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  await enqueue({
    topic: 'users.fetched',
    data: { users: [] }
  })

  return {
    status: 200,
    body: { message: 'Users retrieved' }
  }
}
```

### Example 2: Python Queue Step

```python title="src/data-processor_step.py"
from datetime import datetime, timezone
from motia import enqueue

config = {
    "name": "data-processor",
    "description": "Process incoming data with Python",
    "triggers": [
        {"type": "queue", "topic": "users.fetched"}
    ],
    "enqueues": ["data.processed"],
    "flows": ["user-management"]
}

async def handler(input_data):
    processed_data = {
        "original": input_data,
        "processed_at": datetime.now(timezone.utc).isoformat(),
        "count": len(input_data.get("users", []))
    }

    await enqueue({
        "topic": "data.processed",
        "data": processed_data
    })
```

### Example 3: JavaScript Automation Step

```javascript title="src/send-notifications.step.js"
import { enqueue, logger } from 'motia'

export const config = {
  name: 'send-notifications',
  description: 'Send notifications via multiple channels',
  triggers: [
    { type: 'queue', topic: 'data.processed' },
  ],
  enqueues: ['notifications.sent'],
  flows: ['user-management']
}

export const handler = async (input) => {
  logger.info('Sending notifications', { data: input })

  const results = await Promise.all([
    sendEmail(input),
    sendSMS(input),
    sendPush(input)
  ])

  await enqueue({
    topic: 'notifications.sent',
    data: {
      results,
      sent_at: new Date().toISOString()
    }
  })
}

async function sendEmail(data) { /* implementation */ }
async function sendSMS(data) { /* implementation */ }
async function sendPush(data) { /* implementation */ }
```

## Auto-Generated Files

Some files in your Motia project are automatically generated:

- `types.d.ts` - TypeScript generates this for type definitions

## Discovery Troubleshooting

If Motia isn't discovering your steps:

### Common Issues

<Tabs items={["Filename Issues", "Export Issues", "Location Issues"]}>
<Tab value="Filename Issues">

**Missing `.step.` (or `_step` for Python) in filename**

<div className="grid grid-cols-1 md:grid-cols-2 gap-4">
<div>
Won't be discovered:
<Folder name="src" defaultOpen>
  <File name="user-handler.ts" />
  <File name="data-processor.py" />
  <File name="webhook.js" />
</Folder>
</div>
<div>
Will be discovered:
<Folder name="src" defaultOpen>
  <File name="user-handler.step.ts" />
  <File name="data-processor_step.py" />
  <File name="webhook.step.js" />
</Folder>
</div>
</div>

</Tab>
<Tab value="Export Issues">

**Missing config export**

```typescript title="Won't be discovered"
export const handler = async () => {
  console.log('This won't be found by Motia')
}
```

```typescript title="Will be discovered"
import { type Handlers, type StepConfig } from 'motia'

export const config = {
  name: 'my-step',
  description: 'A queue handler step',
  triggers: [
    { type: 'queue', topic: 'my-topic' },
  ],
  enqueues: ['my-output'],
  flows: ['my-flow'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (input, ctx) => {
  // Motia will discover and register this step
}
```

</Tab>
<Tab value="Location Issues">

**File outside src/ directory**

<div className="grid grid-cols-1 md:grid-cols-2 gap-4">
<div>
Won't be discovered:
<Folder name="project-root" defaultOpen>
  <Folder name="lib">
    <File name="user-handler.step.ts" />
  </Folder>
  <Folder name="utils">
    <File name="processor_step.py" />
  </Folder>
  <File name="helper.step.js" />
</Folder>
</div>
<div>
Will be discovered:
<Folder name="project-root" defaultOpen>
  <Folder name="src" defaultOpen>
    <File name="user-handler.step.ts" />
    <Folder name="services">
      <File name="processor_step.py" />
    </Folder>
  </Folder>
</Folder>
</div>
</div>

**Note:** Steps must be located within the `src/` directory to be discovered. Files can be nested at any depth within the `src/` directory.

</Tab>
</Tabs>

### Discovery Verification

Check if your steps are discovered:

```bash
npm run dev

# Look for step creation in your console:
# [CREATED] Step (Cron) src/petstore/state-audit-cron.step.ts created
# [CREATED] Step (Queue) src/petstore/process-food-order.step.ts created
# [CREATED] Step (Queue) src/petstore/notification.step.ts created
# [CREATED] Step (HTTP) src/petstore/api.step.ts created
```

## Next Steps

Now that you understand how Motia discovers and organizes steps:

- Learn about [Core Concepts](/docs/concepts) to understand how steps work together
- Explore [Defining Steps](/docs/concepts/steps) for detailed step creation
- Check out [Triggers](/docs/concepts/steps#triggers) for API, Queue, and Cron steps
