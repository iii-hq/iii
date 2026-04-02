<a href="https://motia.dev">
  <img src="assets/github-readme-banner.png" alt="Motia Banner" width="100%">
</a>

<p align="center">
  <a href="https://trendshift.io/repositories/14032" style="margin-right:8px;">
    <img src="https://trendshift.io/api/badge/repositories/14032" alt="Motia" style="width: 250px; height: 55px; margin-right:8px;" width="250" height="55"/>
  </a>
  <a href="https://vercel.com/blog/summer-2025-oss-program#motia" target="_blank" style="margin-left:8px;">
    <img alt="Vercel OSS Program" src="https://vercel.com/oss/program-badge.svg" style="width: 250px; height: 55px; margin-left:8px;" width="250" height="55"/>
  </a>
</p>

<p align="center">
  <strong>🔥 The Unified Backend Framework That Eliminates Runtime Fragmentation 🔥</strong>
</p>
<p align="center">
  <em>APIs, background jobs, queueing, streaming, states, workflows, AI agents, observability, scaling, and deployment all in one system. JavaScript, TypeScript, Python, and more in a single core primitive</em>
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/motia">
    <img src="https://img.shields.io/npm/v/motia?style=flat&logo=npm&logoColor=white&color=CB3837&labelColor=000000" alt="npm version">
  </a>
  <a href="https://github.com/MotiaDev/motia/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-Apache%202.0-blue?style=flat&logo=apache&logoColor=white&labelColor=000000" alt="Apache 2.0 License">
  </a>
  <a href="https://github.com/MotiaDev/motia">
    <img src="https://img.shields.io/github/stars/MotiaDev/motia?style=flat&logo=github&logoColor=white&color=yellow&labelColor=000000" alt="GitHub stars">
  </a>
  <a href="https://twitter.com/motiadev" target="_blank">
    <img src="https://img.shields.io/badge/Follow-@motiadev-1DA1F2?style=flat&logo=twitter&logoColor=white&labelColor=000000" alt="Twitter Follow">
  </a>
  <a href="https://discord.gg/motia" target="_blank">
    <img src="https://img.shields.io/discord/1322278831184281721?style=flat&logo=discord&logoColor=white&color=5865F2&label=Discord&labelColor=000000" alt="Discord">
  </a>
</p>

<p align="center">
  <a href="https://www.motia.dev/manifesto">💡 Motia Manifesto</a> •
  <a href="https://www.motia.dev/docs/getting-started/quick-start">🚀 Quick Start</a> •
  <a href="https://www.motia.dev/docs/concepts/steps">📋 Defining Steps</a> •
  <a href="https://www.motia.dev/docs">📚 Docs</a>
</p>

---

## 🚀 Create your first Motia App

Get started in seconds:

```bash
npx motia@latest create
```

---

## 🎯 What is Motia?

Backend development today is fragmented.

APIs live in one framework, background jobs in another, queues and schedulers elsewhere, and now AI agents and streaming systems have their own runtimes. Add observability and state management on top, and you're stitching together half a dozen tools before writing your first feature.

**Motia unifies all of these concerns around one core primitive: the Step.**

Just as React made frontend development simple by introducing components, Motia redefines backend development with Steps - a single primitive that handles everything.

Every backend pattern, API endpoints, background jobs, queues, workflows, AI agents, streaming, observability, and state, is expressed with the same primitive.

To read more about this, check out our **[manifesto](https://motia.dev/manifesto)**.

---

## The Core Primitive: the Step

A Step is just a file with a `config` and a `handler`. Motia auto-discovers these files and connects them automatically.

Here's a simple example of two Steps working together: an API Step that enqueues an event, and an Event Step that processes it.

<details open>
<summary><b>TypeScript</b></summary>

```ts
// steps/send-message.step.ts
export const config = {
  name: 'SendMessage',
  triggers: [
    {
      type: 'http',
      method: 'POST',
      path: '/messages',
    }
  ],
  enqueues: ['message.sent']
};

export const handler = async (req, { enqueue }) => {
  await enqueue({
    topic: 'message.sent',
    data: { text: req.body.text }
  });
  return { status: 200, body: { ok: true } };
};
```

```ts
// steps/process-message.step.ts
export const config = {
  name: 'ProcessMessage',
  triggers: [
    {
      type: 'queue',
      topic: 'message.sent',
    }
  ],
};

export const handler = async (input, { logger }) => {
  logger.info('Processing message', input);
};
```

</details>

<details>
<summary><b>JavaScript</b></summary>

```js
// steps/send-message.step.js
const config = {
  name: 'SendMessage',
  triggers: [
    {
      type: 'http',
      method: 'POST',
      path: '/messages',
    }
  ],
  enqueues: ['message.sent']
};

const handler = async (req, { enqueue }) => {
  await enqueue({
    topic: 'message.sent',
    data: { text: req.body.text }
  });
  return { status: 200, body: { ok: true } };
};

module.exports = { config, handler };
```

```js
// steps/process-message.step.js
const config = {
  name: 'ProcessMessage',
  triggers: [
    {
      type: 'queue',
      topic: 'message.sent',
    }
  ],
};

const handler = async (input, { logger }) => {
  logger.info('Processing message', input);
};

module.exports = { config, handler };
```

</details>

👉 With just two files, you've built an **API endpoint**, a **queue**, and a **worker**. No extra frameworks required.

**[Learn more about Steps →](https://motia.dev/docs/concepts/steps)**

[![Motia combines APIs, background queues, and AI agents into one system](assets/github-readme-banner.gif)](https://motia.dev)

## 💻 Remix your own Motia App in Replit
[![Open in Replit](https://img.shields.io/badge/Open%20in-Replit-blue?logo=replit&style=for-the-badge)](https://replit.com/@motiadev/motia)

## 🚀 Quickstart

Get Motia project up and running in **under 60 seconds**:

### 1. Bootstrap a New Motia Project

```bash
npx motia@latest create   # runs the interactive terminal
```

Follow the prompts to pick a template, project name, and language.
![motia-terminal](assets/motia-terminal.gif)

### 2. Start the Workbench

Inside your new project folder, launch the dev server:

```bash
npm run dev # ➜ http://localhost:3000
```

**That's it!** You have:
- ✅ REST APIs with validation
- ✅ Visual debugger & tracing  
- ✅ Multi-language support
- ✅ Event-driven architecture
- ✅ Zero configuration
- ✅ AI development guides included (Cursor, OpenCode, Codex, and more)

![new-workbench](assets/new-workbench.png)

> 📖 **[Full tutorial in our docs →](https://motia.dev/docs/getting-started/quick-start)**

### 🤖 AI-Assisted Development

Every Motia project includes detailed AI development guides that work with **any AI coding tool**:

- **[Cursor IDE](https://cursor.sh/)** - Optimized `.mdc` rules with context-aware suggestions
- **[OpenCode](https://opencode.ai/)**, **[Codex (OpenAI)](https://openai.com/index/introducing-codex/)** - Full support via `AGENTS.md` standard
- **Aider, Jules, Factory, Amp, GitHub Copilot** - Compatible with [AGENTS.md format](https://agents.md/) (used by 20k+ projects)

The guides include patterns for API endpoints, background tasks, state management, real-time streaming, and complete architecture blueprints.

> 🤖 **[Learn more about AI development support →](https://motia.dev/docs/ai-development-guide)**

## 🎯 Triggers

| Type | When it runs | Use Case |
|------|--------------|----------|
| **`http`** | HTTP Request | REST endpoints |
| **`queue`** | Queue subscription | Background processing |  
| **`cron`** | Schedule | Recurring jobs |
| **`state`** | State change | State management |
| **`stream`** | Stream subscription | Real-time streaming |

> 📖 **[Learn more about Steps →](https://motia.dev/docs/concepts/steps)**

---

## 🎯 Examples

### 🏆 **[ChessArena.ai](https://chessarena.ai)** - Full-Featured Production App

A complete chess platform benchmarking LLM performance with real-time evaluation.

**[Live Website →](https://chessarena.ai)** | **[Source Code →](https://github.com/MotiaDev/chessarena-ai)**

> ![ChessArena.ai in action (raw GIF)](https://github.com/MotiaDev/chessarena-ai/blob/main/public/images/chessarena.gif?raw=true)

**Built from scratch to production deployment, featuring:**
- 🔐 **Authentication & user management**
- 🤖 **Multi-agent LLM evaluation** (OpenAI, Claude, Gemini, Grok)
- 🐍 **Python engine integration** (Stockfish chess evaluation)
- 📊 **Real-time streaming** with live move updates and scoring
- 🎨 **Modern React UI** with interactive chess boards
- 🔄 **Event-driven workflows** connecting TypeScript APIs to Python processors
- 📈 **Live leaderboards** with move-by-move quality scoring
- 🚀 **Production deployment** on Motia Cloud

### 📚 **More Examples**

**[View all 20+ examples →](https://github.com/MotiaDev/motia-examples)**

| Example | Description |
|---------|-------------|
| **[AI Research Agent](https://github.com/MotiaDev/motia-examples/tree/main/examples/ai-deep-research-agent)** | Web research with iterative analysis |
| **[Streaming Chatbot](https://github.com/MotiaDev/motia-examples/tree/main/examples/streaming-ai-chatbot)** | Real-time AI responses |
| **[Gmail Automation](https://github.com/MotiaDev/motia-examples/tree/main/examples/gmail-workflow)** | Smart email processing |
| **[GitHub PR Manager](https://github.com/MotiaDev/motia-examples/tree/main/examples/github-integration-workflow)** | Automated PR workflows |
| **[Finance Agent](https://github.com/MotiaDev/motia-examples/tree/main/examples/finance-agent)** | Real-time market analysis |

**Features demonstrated:** Multi-language workflows • Real-time streaming • AI integration • Production deployment

---

## 🌐 Language Support

| Language | Status | 
|----------|--------|
| **JavaScript** | ✅ Stable |
| **TypeScript** | ✅ Stable |
| **Python** | ✅ Stable |
| **Ruby** | 🚧 Beta |
| **Go** | 🔄 Soon |

## 📚 Resources

- **[📖 Documentation](https://motia.dev/docs)** - Complete guides and API reference
- **[💬 Discord](https://discord.gg/motia)** - Community support and discussions
- **[🐛 GitHub Issues](https://github.com/MotiaDev/motia/issues)** - Bug reports and feature requests
- **[🗺️ Roadmap](https://github.com/orgs/MotiaDev/projects/2)** - Upcoming features and progress

## 🚧 Roadmap

We have a public roadmap for Motia, you can view it [here](https://github.com/orgs/MotiaDev/projects/2/views/4).

Feel free to add comments to the issues, or create a new issue if you have a feature request.

| Feature | Status | Link | Description |
| ------- | ------ | ---- | ----------- |
| Streams: RBAC | ✅ Shipped | [#495](https://github.com/MotiaDev/motia/issues/495) | Add support for RBAC |
| Streams: Workbench UI | ✅ Shipped | [#497](https://github.com/MotiaDev/motia/issues/497) | Add support for Workbench UI |
| Queue Strategies | ✅ Shipped | [#476](https://github.com/MotiaDev/motia/issues/476) | Add support for Queue Strategies |
| Reactive Steps | ✅ Shipped | [#477](https://github.com/MotiaDev/motia/issues/477) | Add support for Reactive Steps |
| Point in time triggers | 📅 Planned | [#480](https://github.com/MotiaDev/motia/issues/480) | Add support for Point in time triggers |
| Workbench plugins | ✅ Shipped | [#481](https://github.com/MotiaDev/motia/issues/481) | Add support for Workbench plugins |
| Rewrite core in Rust | ✅ Shipped | [#482](https://github.com/MotiaDev/motia/issues/482) | Rewrite our Core in Rust |
| Decrease deployment time | ✅ Shipped | [#483](https://github.com/MotiaDev/motia/issues/483) | Decrease deployment time |
| Built-in database support | 📅 Planned | [#484](https://github.com/MotiaDev/motia/issues/484) | Add support for built-in database |

## 🤝 Contributing

We welcome contributions! Check our **[Contributing Guide](https://github.com/MotiaDev/motia/blob/main/CONTRIBUTING.md)** to get started.

---

<div align="center">

**[🚀 Get Started](https://motia.dev)** • **[📖 Docs](https://motia.dev/docs)** • **[💬 Discord](https://discord.gg/motia)**

<a href="https://git-history.com">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://git-history.com/api/embed/stars?repos=MotiaDev/motia&theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://git-history.com/api/embed/stars?repos=MotiaDev/motia&theme=light" />
    <img alt="Star History Chart" src="https://git-history.com/api/embed/stars?repos=MotiaDev/motia&theme=dark" width="100%" />
  </picture>
</a>

<sub>⭐ **Star us if you find Motia useful!**</sub>

</div>
 