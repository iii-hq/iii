// Every outbound link and shared string on the site, in one place. Components
// import from here instead of hard-coding URLs.
export const site = {
  name: "iii",
  url: "https://iii.dev",
  title: "iii — Three primitives. Zero integration cost.",
  description:
    "The latest docs on how to use iii, a new paradigm to effortlessly compose, extend, and observe every service in any system in real-time for the first time ever.",
  keywords: [
    "iii",
    "worker",
    "trigger",
    "function",
    "three primitives",
    "distributed systems",
    "durable execution",
    "polyglot runtime",
    "AI agents",
    "agent runtime",
    "TypeScript",
    "Python",
    "Rust",
    "WebSocket",
    "interoperability",
    "live discovery",
    "live observability",
    "queues",
    "cron",
    "http",
    "state",
    "streams",
    "sandbox",
  ],
} as const

export const links = {
  docs: "https://iii.dev/docs",
  install: "https://iii.dev/docs/install",
  quickstart: "https://iii.dev/docs/quickstart",
  manifesto: "/manifesto",
  blog: "/blog",
  roadmap: "/roadmap",
  privacy: "/privacy-policy",
  workerRegistry: "https://workers.iii.dev",
  github: "https://github.com/iii-hq/iii",
  discord: "https://discord.gg/iiidev",
  twitter: "https://x.com/iiidevs",
  linkedin: "https://www.linkedin.com/company/iii-dev",
} as const

/** The command the install.sh buttons copy. */
export const INSTALL_COMMAND = "curl -fsSL https://install.iii.dev/iii/main/install.sh | sh"

/** The self-routing prompt behind every "copy prompt" button. */
export const COPY_PROMPT_TEXT = [
  "Read https://iii.dev/llms.txt and https://iii.dev/AGENTS.md.",
  "If you're a chat assistant, use llms.txt to explain iii to me —",
  "the three primitives (Worker, Trigger, Function) and how it",
  "compares to my stack. If you're a coding agent, use AGENTS.md to",
  "install iii and build the Quickstart with me",
  "(https://iii.dev/docs/quickstart), running it locally.",
].join("\n")

/** The question behind the footer's "Ask about iii on" links. Short: it's a URL parameter. */
export const ASK_AI_PROMPT =
  "Read https://iii.dev/llms.txt and explain iii to me: its three primitives (Worker, Trigger, Function), what it replaces in a typical backend, and how I would install it and build a first service."

/** Chat assistants that accept a prefilled question in the URL. `url(q)` takes the encoded prompt. */
export const ASK_AI_ASSISTANTS = [
  { id: "chatgpt", name: "ChatGPT", url: (q: string) => `https://chatgpt.com/?q=${q}` },
  { id: "claude", name: "Claude", url: (q: string) => `https://claude.ai/new?q=${q}` },
  { id: "perplexity", name: "Perplexity", url: (q: string) => `https://www.perplexity.ai/search?q=${q}` },
  { id: "grok", name: "Grok", url: (q: string) => `https://grok.com/?q=${q}` },
] as const
