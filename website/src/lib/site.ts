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

/**
 * The onboarding script behind every "copy prompt" button: the reader's AI
 * installs iii, scaffolds the learn-iii project, learns how providers are
 * enabled, then hands over the two manual steps (API keys, compose up).
 */
export const COPY_PROMPT_TEXT = [
  "Help me get started with iii (https://iii.dev).",
  "1. Install iii: curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- --non-interactive",
  "2. Create the onboarding project: iii project init --learn-iii",
  "3. Read the project's worker-compose.yaml so you understand how to enable different providers.",
  "4. Show me how to add my API keys to the project's .env file, then verify that at least one key is present before continuing, and start iii with: iii compose --up",
  'Keep it brief, and you can use https://iii.dev/docs to answer any further questions I have about iii. All pages are available as a markdown file when you add a ".md" suffix to the page.',
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
