---
title: Quick Start
description: Get up and running with a new Motia project in just a few seconds.
---

Motia is a unified backend framework where everything is a **Step** — a file with a config and a handler. Python developers don't need npm; Node.js developers don't need Python. Choose your language or use both in a mixed project. In this guide you will create a new project with the motia-cli, start it with the iii engine, and trigger your first workflow.

## Prerequisites

<Tabs items={['Node.js', 'Python', 'Mixed']}>
  <Tab value="Node.js">
    - **Git** installed
    - **Node.js** 18+
    - **iii** (install in Step 1)
  </Tab>
  <Tab value="Python">
    - **Git** installed
    - **Python** 3.10+
    - **[uv](https://docs.astral.sh/uv/)** for Python package management
    - **iii** (install in Step 1)
  </Tab>
  <Tab value="Mixed">
    - **Git** installed
    - **Node.js** 18+
    - **Python** 3.10+
    - **[uv](https://docs.astral.sh/uv/)**
    - **iii** (install in Step 1)
  </Tab>
</Tabs>

<Steps>

<Step>
### 1. Install iii

The iii engine is the runtime that powers Motia. Install it with:

```bash
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh
```

Verify the installation:

```bash
iii -v
```

</Step>

<Step>
### 2. Install motia-cli

Install the Motia project scaffolding CLI:

```bash
curl -fsSL https://raw.githubusercontent.com/MotiaDev/motia-cli/main/install.sh | sh
```

Or via Homebrew:

```bash
brew tap MotiaDev/tap
brew install motia-cli
```

</Step>

<Step>
### 3. Create Your Project

Run the create command and follow the prompts to choose your project name and language:

```bash
motia-cli create my-project
```

Select your preferred language when prompted:

- **Node.js** — TypeScript/Node.js only (no Python required)
- **Python** — Python only (no npm required)
- **Mixed** — Node.js handles HTTP APIs, Python handles background processing; both share the same iii infrastructure

</Step>

<Step>
### 4. Start the Project

The `iii-config.yaml` in the project tells the iii engine how to run Motia — starting iii starts everything.

```bash
cd my-project
iii -c iii-config.yaml
```

</Step>

<Step>
### 5. Run Your First Flow

This example is a ticketing system for user issues. Try it out:

```bash
# Create a ticket
curl -X POST http://localhost:3111/tickets \
  -H "Content-Type: application/json" \
  -d '{"title":"Login issue","description":"User is having trouble creating an account","priority":"high","customerEmail":"user@example.com"}' | jq
```

```bash
# List all tickets
curl http://localhost:3111/tickets | jq
```

</Step>

<Step>
### 6. What Just Happened?

When you created a ticket, here is what Motia did behind the scenes:

1. **HTTP trigger** — The `POST /tickets` request hit a Step with an `http` trigger, which validated the input and stored the ticket in state.
2. **Queue trigger** — That Step enqueued a message to a topic. Another Step with a `queue` trigger picked it up and processed the ticket asynchronously (e.g., classification, notification).
3. **State** — The ticket data was persisted using Motia's built-in key-value state, so the `GET /tickets` endpoint could retrieve it.

<Tabs items={['Node.js', 'Python', 'Mixed']}>
  <Tab value="Node.js">

All of this was defined in simple Step files inside the `src/` folder. Each one uses the `.step.ts` pattern and exports a `config` (defining triggers and topics) and a `handler` (business logic).

  </Tab>
  <Tab value="Python">

All of this was defined in simple Step files inside the `steps/` folder. Each one uses the `_step.py` pattern, defines a `config` dict (triggers and topics), and an `async def handler` (business logic).

  </Tab>
  <Tab value="Mixed">

In the mixed template, Node.js handles the HTTP API endpoints (`create-ticket`, `list-tickets`) in `nodejs/src/`, while Python handles queue and cron triggers (`triage`, `notify`, `sla-monitor`, `escalate`) in `python/steps/`. Both run as subprocesses connected to the same iii infrastructure.

  </Tab>
</Tabs>

</Step>

<Step>
### 7. Try the iii Console

<Callout title="iii is in alpha" type="warn">
  Motia and iii are under very active development. We will be regularly updating Motia, iii, and the iii console to address any issues.
</Callout>

The iii console gives you complete observability of your Motia project — flow diagrams, real-time logs, state inspection, and stream monitoring. Install and start it in a new terminal:

```bash
curl -fsSL https://install.iii.dev/console/main/install.sh | sh
iii-console --enable-flow
```

Then open your browser to: [http://localhost:3113/](http://localhost:3113/)

The console dashboard gives you a system overview of your running application:

![iii Console Dashboard](/console/dashboard.png)

Navigate to the **Flow** tab to see your Steps connected as a visual workflow:

![Flow diagram in iii Console](/console/flow-view.png)

</Step>

<Step>
### Manual Setup (Alternative)

If you prefer to clone the example repository directly instead of using motia-cli:

<Tabs items={['Node.js', 'Python', 'Mixed']}>
  <Tab value="Node.js">

```bash
git clone https://github.com/MotiaDev/motia-iii-example.git
cd motia-iii-example/nodejs
npm install
```

  </Tab>
  <Tab value="Python">

```bash
git clone https://github.com/MotiaDev/motia-iii-example.git
cd motia-iii-example/python
uv sync
```

  </Tab>
  <Tab value="Mixed">

```bash
git clone https://github.com/MotiaDev/motia-iii-example.git
cd motia-iii-example/mixed
(cd nodejs && npm install)
(cd python && uv sync)
```

  </Tab>
</Tabs>

</Step>

<Step>
### Next Steps

You have successfully run your first Motia workflow. Here is where to go next:

<Cards>
  <Card title="Core Concepts" href="/docs/concepts/overview">
    Understand Steps, triggers, and the event-driven architecture that powers Motia.
  </Card>
  <Card title="Examples" href="/docs/examples">
    Explore real-world examples covering APIs, AI agents, workflows, and more.
  </Card>
  <Card title="The iii Engine" href="/docs/concepts/iii-engine">
    Learn how iii manages infrastructure through config.yaml modules.
  </Card>
</Cards>

</Step>

</Steps>
