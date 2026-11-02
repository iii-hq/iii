export const links = {
  quickstart: 'https://iii.dev/docs/quickstart',
  install: 'https://iii.dev/docs/install',
  /* Served by this app (app/(site)/manifesto, blog, roadmap, benchmarks, privacy-policy). */
  manifesto: '/manifesto',
  docs: 'https://iii.dev/docs',
  registry: 'https://workers.iii.dev/',
  blog: '/blog',
  roadmap: '/roadmap',
  benchmarks: '/benchmarks',
  github: 'https://github.com/iii-hq/iii',
  workers: 'https://github.com/iii-hq/workers',
  discord: 'https://discord.gg/iiidev',
  x: 'https://x.com/iiidevs',
  linkedin: 'https://www.linkedin.com/company/iii-dev',
  agentGuide: 'https://iii.dev/docs/quickstart',
  privacy: '/privacy-policy',
} as const

export const installCommand = 'curl -fsSL https://install.iii.dev/iii/main/install.sh | sh'

const askPrompt =
  'Read https://iii.dev/llms.txt and explain iii to me: its three primitives (Worker, Trigger, Function), what it replaces in a typical backend, and how I would install it and build a first service.'

/** Assistants that accept a prefilled question in the URL, each opened on `prompt`. */
export const askAssistants = (prompt: string) => {
  const q = encodeURIComponent(prompt)
  return [
    { id: 'chatgpt', label: 'ChatGPT', href: `https://chatgpt.com/?q=${q}` },
    { id: 'claude', label: 'Claude', href: `https://claude.ai/new?q=${q}` },
    { id: 'perplexity', label: 'Perplexity', href: `https://www.perplexity.ai/search?q=${q}` },
    { id: 'grok', label: 'Grok', href: `https://grok.com/?q=${q}` },
  ] as const
}

export type AssistantId = ReturnType<typeof askAssistants>[number]['id']

/** The footer's assistants; the question points them at llms.txt. */
export const askAbout = askAssistants(askPrompt)
