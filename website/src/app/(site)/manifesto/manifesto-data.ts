// The manifesto, word for word from the pre-Astro manifesto.html (see
// website/src/components/manifesto/body.html). The voice is lowercase on
// purpose; keep it that way. Only the design lives in page.tsx.

/** A run of body text. `{ i }` is an italic phrase (`<i>` in the source). */
export type Inline = string | { i: string }

export type Statement = {
  /** the section id: an anchor, and the heading id for `aria-labelledby` */
  id: string
  /** the quieter first phrase of the title */
  accent: string
  /** render the accent phrase as code (Geist Mono) */
  code?: boolean
  /** the heavier second phrase of the title */
  heavy: string
  /** an optional ghosted third phrase (only the closer has one) */
  ghost?: string
  body: Inline[]
}

export const hero = {
  problem: "software complexity continues to grow.",
  verb: "remove it.",
  answer: "add a worker.",
} as const

export const statements: Statement[] = [
  {
    id: "integrations",
    accent: "systems engineering",
    heavy: "is integrations.",
    body: [
      "and integrations have been growing in complexity for forty years. every new capability arrives as its own product category, with its own semantics, its own operational model, and its own integration tax against everything already in your stack. we believe this pattern is ending. a paradigm shift is here, and we are building for it.",
    ],
  },
  {
    id: "three-primitives",
    accent: "three primitives",
    heavy: "are enough.",
    body: [
      "worker. trigger. function. a worker is any process that joins the system. a trigger is anything that causes work to run. a function is the unit of work. every capability (queues, cron, streaming, sandboxing, observability, agents, business logic, devices, UIs, and everything not yet imagined) is built from these three things. the primitives are small enough to hold in your head. everything else is a property of the engine, or a worker someone published.",
    ],
  },
  {
    id: "paradigms",
    accent: "paradigms win",
    heavy: "by collapsing categories.",
    body: [
      "unix won on ",
      { i: "everything is a file" },
      ". react won on ",
      { i: "everything is a component" },
      ". http won on ",
      { i: "request and response" },
      ". every paradigm that compounded for decades did so because the primitive was small enough to fit in your head. iii makes that bet for the entire software stack.",
    ],
  },
  {
    id: "have-a-need",
    accent: "have a need?",
    heavy: "add a worker.",
    body: [
      "need a queue? add a worker. need real-time streaming, scheduling, sandboxing, observability, an agent, a CRM integration, a browser tab as a first-class participant? add a worker. some workers are deterministic code. some are stochastic agents. most real systems are a mix. the semantics live in the functions, not in the category.",
    ],
  },
  {
    id: "quadratic-to-linear",
    accent: "quadratic",
    heavy: "to linear.",
    body: [
      "four services means six possible integrations. twenty means 190. each new addition compounds the coordination cost of everything that came before. iii ends this. a new worker joins by opening a single connection. adding four workers, or twenty, takes the same effort: zero. each worker is immediately available to every other worker the moment it registers. the architecture stops paying compound interest on complexity.",
    ],
  },
  {
    id: "same-contract",
    accent: "same contract,",
    heavy: "both sides of the house.",
    body: [
      "application teams register functions and declare triggers, focused entirely on business logic. platform teams publish workers, focused entirely on the capabilities they provide. both sides fulfill the exact same contract. no bespoke SDKs. no internal client libraries. no per-service API contracts. no integration consulting. the work that used to live between teams disappears.",
    ],
  },
  {
    id: "live-by-default",
    accent: "live",
    heavy: "by default.",
    body: [
      "when a worker registers, its functions become visible and callable across the entire system in real time. when it unregisters, they are gone. traces span every worker, across every language, across every process. the engine is the registry. the protocol is the wire. observability is the wire protocol. there are no stale specs, no propagation delays, and no disconnected log streams to grep across.",
    ],
  },
  {
    id: "one-system",
    accent: "any language. any runtime.",
    heavy: "one system.",
    body: [
      "a typescript worker calls a python worker the same way it calls another typescript worker. the engine handles serialization and routing. a worker in docker, on kubernetes, on the edge, in a browser tab, on a raspberry pi, or inside a hardware-isolated microVM is the same kind of worker. moving a workload is a redeploy, not a rewrite.",
    ],
  },
  {
    id: "one-mental-model",
    accent: "humans and agents share",
    heavy: "one mental model.",
    body: [
      "a new engineer is productive on day one because their mental model never changes from one capability to the next. ai agents can reliably reason about an entire system in a single context window because there is one set of primitives to learn and one always-accurate source of truth for what exists. as agents do more of the work of building and operating software, small primitives compound: easier to onboard, cheaper to prompt, faster to extend, simpler to maintain.",
    ],
  },
  {
    id: "agents-are-workers",
    accent: "agents",
    heavy: "are workers.",
    body: [
      "an agent's tools are functions. its memory is state. its orchestration is triggers. the harness is the system. an agent that hits a task outside its current capabilities can register a worker at runtime and use its functions immediately. this is the difference between scripted LLM calls and real autonomy.",
    ],
  },
  {
    id: "npm-moment",
    accent: "iii worker add",
    code: true,
    heavy: "is the npm moment for systems.",
    body: [
      "what you install is not a library. it is a running participant. a queue worker. a sandbox worker. a classifier. a CRM integration. one command, complete capability, immediately available to every other worker in the system. capabilities that used to require vendor evaluation, procurement, and integration become a single line.",
    ],
  },
  {
    id: "add-a-worker",
    accent: "add a worker.",
    heavy: "add a worker.",
    ghost: "add a worker.",
    body: [
      "AI is the forcing function. simplicity beats complexity. the category-shopping era of software engineering (evaluate, procure, integrate, repeat) is ending. what replaces it is one protocol, three primitives, and a registry of workers that compose without coordination. anything can participate. everything composes. software becomes unreasonably simple to create, extend, and modify.",
    ],
  },
]
