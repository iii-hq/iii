---
title: Why Motia?
description: "Learn why Motia exists — a unified backend framework that replaces juggling separate systems for APIs, queues, workflows, and more."
---

<video controls className="mb-8 w-full rounded-xl" poster="https://assets.motia.dev/images/gifs/v1/1-motia-welcome.gif">
  <source src="https://assets.motia.dev/videos/mp4/site/v1/1-motia-welcome.mp4" type="video/mp4" />
</video>

## Why Motia?

**Build production-grade backends with a single primitive.**

Modern backends shouldn't require juggling frameworks, queues, and services. Motia unifies everything: API endpoints, background jobs, durable workflows, AI agents, streaming, and observability into one runtime with a single core primitive.

**Motia** is your complete backend solution:
- 🌐 **API** - RESTful endpoints with validation and routing
- ⚡ **Background Jobs** - Async processing with built-in queues
- 🔄 **Durable Workflows** - Complex multi-step orchestration
- 🤖 **Agentic** - AI agent workflows with streaming support
- 🏪 **State** - Built-in persistent storage across Steps
- 📊 **Streaming** - Real-time data updates to clients
- 📝 **Logging** - Structured, traceable logs
- 👁️ **Observability** - End-to-end tracing and monitoring

Just as React made frontend development simple by introducing components, **Motia redefines backend development with Steps** - a single primitive that powers everything.

To read more about this, check out our [manifesto](/manifesto).

---

## The Core Primitive: the Step

At the heart of Motia is a single primitive: the **Step** — a file with a `config` and a `handler`. Motia auto-discovers these files and connects them automatically.

Learn more about Steps: [What is a Step?](/docs/concepts/steps)

---

## Working with Multiple Languages

The rapid advancement of AI has reshaped the software industry — many cutting-edge AI tools are available only in specific programming languages, forcing companies to choose between changing their team's skillset or not leveraging these technologies at all.

Motia removes this limitation by allowing each Step to be written in any language, while still sharing a common state.

![Multi-language](./img/what-is-motia/multi-language.png)

_Each rectangle in the diagram above represents a Step, some of them are in TypeScript and others in Python._

Learn more: [Multi-language Support](/docs/concepts/overview#multi-language-support)

---

## Scalability

One of the biggest dilemmas in backend development is choosing between scalability and development velocity. In startup environments, speed often takes priority, resulting in systems that don't scale well and become problematic under increased load.

Motia addresses scalability by leveraging the core primitive of **Steps**: Each step can scale independently avoiding the bottlenecks common in monolithic architectures.

![Scalable](./img/what-is-motia/scalable.png)

---

## Observability

Observability in traditional backends often demands significant engineering effort to implement logging, alerting, and tracing. Typically, these tools are only configured for cloud environments, local development is generally neglected — leading to low productivity and poor dev experience.

Motia offers a complete observability toolkit available in both cloud and local environments.

Learn more: [Observability](/docs/development-guide/observability)

---

## Fault Tolerance

With the rise of AI, many backend tasks have become less deterministic and more error-prone. These scenarios require robust error handling and retry mechanisms. In traditional systems, developers often need to set up and maintain queue infrastructures to ensure resilience, especially when dealing with unreliable responses from LLMs.

Motia provides fault tolerance out of the box, eliminating the need to manually spin up queue infrastructure.

- Using queue triggers, you get retry mechanisms out of the box
- Queue infrastructure configuration is handled by the iii engine through `config.yaml`

---

## Building and Shipping

Building and deploying backends is inherently complex — especially in polyglot environments. Shipping production systems requires tight collaboration between developers and operations, and automation often takes weeks to get right.

Beyond that, cloud provider lock-in, complicated deployment strategies (e.g., rollbacks, blue/green deployments), and a lack of deployment tooling increase the risk of failure.

Motia abstracts these concerns by providing:

- True cloud-provider agnosticism
- Atomic blue/green deployments and one-click rollbacks via Motia Cloud (canary support coming soon)
- First-class polyglot backend support (currently Node.js and Python, with more on the way)

![Deployments](./img/what-is-motia/deployments.png)

_The image above shows several Steps being built to a single Motia deployable that are ultimately deployed to a cloud provider of your choice.
Currently we're supporting AWS and Kubernetes, more Cloud providers coming soon. Check our [roadmap](https://github.com/orgs/MotiaDev/projects/2/views/4?filterQuery=title%3A+BYOC) for more details._

### Rollbacks and Deployment Strategies

Deploying cloud-native, fault-tolerant applications often involves modifying queue systems and other infrastructure components.
These changes can introduce incompatibilities and lead to runtime failures.

Motia Cloud solves this with **Atomic Deployments**, which:

- Each deployment spins up a new isolated service that shares the same data layer
- Ensures safe, rollback-capable deployments without risking service downtime
- Instant rollbacks with one click since each deployment is isolated

---

## Real-time Data Streaming

Handling real-time data is one of the most common — and complex — challenges in backend development. It's necessary when building event-driven applications, and it typically requires setting up and maintaining a significant amount of infrastructure.

Motia provides what we call _Streams_: Developers define the structure of the data — any changes to these objects are streamed to all subscribed clients in real-time.

![Real-time data streaming](./img/what-is-motia/streams.png)

_The image above shows a Stream definition, a Node.js Step mutating the data and a client subscribing to the stream receiving real-time updates._

Learn more: [Streams](/docs/development-guide/streams)
