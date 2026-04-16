# AI Agents Quickstart

Build an event-driven AI agent system step by step. You'll connect an account
events worker, a sales notification subscriber, an AI agent that analyzes
strategic accounts, and a legacy Java CRM — all through the iii event bus.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                         iii Engine                                │
│         (port 49134 (engine), 3111 (http api))                   │
│                                                                  │
│  topic: account.upgraded                                         │
│  ┌──────────┐    ┌──────────────┐    ┌───────────────────────┐   │
│  │ publish  │───→│ sales::notify│    │ agents::strategic-    │   │
│  │          │───→│              │    │ observer              │   │
│  └──────────┘    └──────────────┘    └───────────────────────┘   │
└──────┬───────────────────────────────────────────┬───────────────┘
       │                                           │
┌──────┴──────┐                            ┌───────┴──────┐
│  account-   │  ← cross-worker trigger →  │   ai-agent   │
│  events     │                            │   (Python)   │
│  (TS)       │                            └──────────────┘
└──────┬──────┘
       │ HTTP proxy
┌──────┴──────┐
│   legacy-   │
│   worker    │
│   (Java)    │
└─────────────┘
```

## Prerequisites

### Required

- **iii engine** installed (see https://iii.dev/docs)
- **Node.js 20+** (for account-events worker)

### Optional (or use Docker)

- **Python 3.10+** (for ai-agent)
- **Java JDK 17+** (for legacy-worker, just `javac` and `java`)
- **Docker** (to run all workers via `docker compose`)

## Quick Start

### 1. Start the iii engine

```bash
iii -c iii-config.yaml
```

### 2. Follow the steps

You can either run all workers at once with Docker, or follow the guided
step-by-step flow below (recommended for learning).

#### Option A: Step-by-step (recommended)

Each worker prints console messages guiding you to the next step.

**Step 1 — Start account-events:**

```bash
cd workers/account-events
npm install
npm run dev
```

Test it:

```bash
curl -X POST http://localhost:3111/account/upgrade \
  -H "Content-Type: application/json" \
  -d '{"companyId":"acme-corp"}'
```

**Step 2 — Uncomment sales notifications** in `workers/account-events/src/worker.ts`:

Find the `STEP 2` block and uncomment it. The file watcher will restart
automatically. Run the curl command again and watch the sales notification
appear in the console.

**Step 3 — Start the AI agent:**

```bash
cd workers/ai-agent
python3 -m venv venv
source venv/bin/activate
pip install -r requirements.txt
python agent.py
```

Run the curl command again. You'll see the AI agent analyze the account in
its console output.

**Step 4 — Start the legacy worker and uncomment the proxy:**

```bash
cd workers/legacy-worker
javac LegacyServer.java
java LegacyServer
```

Then uncomment the `STEP 4` block in `workers/account-events/src/worker.ts`.
Test the legacy endpoint:

```bash
curl -X POST http://localhost:3111/legacy/status \
  -H "Content-Type: application/json" \
  -d '{"companyId":"acme-corp"}'
```

**Step 5 — Add the ARR filter:**

Uncomment the `STEP 5` block in `workers/ai-agent/agent.py`. Now test with
different companies:

```bash
# High-value account (ARR $120k) — will be analyzed
curl -X POST http://localhost:3111/account/upgrade \
  -H "Content-Type: application/json" \
  -d '{"companyId":"acme-corp"}'

# Low-value account (ARR $5k) — will be skipped
curl -X POST http://localhost:3111/account/upgrade \
  -H "Content-Type: application/json" \
  -d '{"companyId":"small-biz"}'
```

**Step 6 (optional) — Enrich with legacy data:**

Uncomment the `STEP 6` block in `workers/ai-agent/agent.py` to have the AI
agent pull historical context from the legacy CRM when analyzing accounts.

#### Option B: Docker Compose

```bash
docker compose up --build
```

This starts all three workers. You'll still want to follow the uncomment
steps in the source files to progressively enable features. Docker will
detect file changes and rebuild automatically.

## What You Learned

| Concept | Where you saw it |
|---------|-----------------|
| **Pub/Sub events** | `publish` + `subscribe` trigger for `account.upgraded` topic |
| **Cross-worker triggers** | AI agent calling `accounts::get-details` in a different worker |
| **HTTP triggers** | `POST /account/upgrade` and `POST /legacy/status` endpoints |
| **Legacy integration** | Java HTTP server connected via iii proxy — no SDK in the legacy code |
| **Multi-language** | TypeScript orchestrator, Python AI agent, Java legacy worker |
| **Observability** | ARR filter fix demonstrates scoping agent behavior with traces |

Visit https://iii.dev/docs/concepts to learn more about the primitives powering iii.
