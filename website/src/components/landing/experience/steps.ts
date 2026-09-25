// The scripted "work with iii" conversation played by the experience sequencer.
// Ported from the Astro site's HOMEPAGE_FLOW (itself from iii-hq/iii
// website/components/demo/homepage-script.ts). `type` values double as the
// `step_type` analytics param, so keep them stable.

export type Sender = { name: string; role?: string }
export type Language = "python" | "typescript"
export type StatusVariant = "info" | "success" | "warn" | "alert" | "accent"
export type StatusIconName = "check" | "connect" | "deploy" | "observe" | "bolt"

type StepBase = {
  id: string
  /** ms of typing dots shown before the step appears */
  delay?: number
}

export type ChatStep = StepBase & {
  type: "slack-message"
  sender: Sender
  content: string
  /** renders the ticket card that starts the flow */
  action?: { label: string; title?: string; count?: number }
  autoAdvance?: number
}

export type ReplyStep = StepBase & {
  type: "reply"
  content: string
  sendLabel?: string
  typingSpeed?: number
  autoAdvance?: number
}

export type CodeStep = StepBase & {
  type: "code-editor"
  filename: string
  language: Language
  lines: readonly string[]
  doneLabel?: string
  lineDelay?: number
  autoAdvance?: number
}

export type StatusStep = StepBase & {
  type: "status"
  variant: StatusVariant
  icon: StatusIconName
  headline: string
  detail?: string
  autoAdvance?: number
}

export type TerminalStep = StepBase & {
  type: "terminal-command"
  command: string
  output: string
  typingSpeed?: number
  autoAdvance?: number
}

export type Trace = { operation: string; duration?: string; status: "ok" | "error" }
export type Span = { label: string; duration: string; widthPercent: number; depth: number; status: "ok" | "error" }

export type TraceStep = StepBase & {
  type: "console-trace"
  traces: readonly Trace[]
  activeTraceIndex: number
  spans: readonly Span[]
  errorSpanIndex?: number
  detail: { status: string; service: string; error?: string }
  autoAdvance?: number
}

export type Step = ChatStep | ReplyStep | CodeStep | StatusStep | TerminalStep | TraceStep
export type StepType = Step["type"]

export const STATUS_ICONS: Record<StatusIconName, string> = {
  check: "✓",
  connect: "⧇",
  deploy: "▲",
  observe: "◉",
  bolt: "⚡",
}

const ALEX: Sender = { name: "Alex", role: "Product lead" }

/** Every timing in the flow is multiplied by this. Below 1 is faster. */
const SPEED = 0.75

const TIMING_KEYS = ["delay", "autoAdvance", "typingSpeed", "lineDelay"] as const

function scaleTimings<T extends Step>(step: T): T {
  const out: Record<string, unknown> = { ...step }
  for (const k of TIMING_KEYS) {
    const v = out[k]
    if (typeof v === "number") out[k] = Math.round(v * SPEED)
  }
  return out as T
}

const RAW_FLOW: readonly Step[] = [
  {
    id: "msg-1",
    type: "slack-message",
    sender: ALEX,
    content: "We need to classify user-submitted content for toxicity before it goes live. Can you set something up?",
    action: { label: "See what it's like to work with iii", title: "Classify user content for toxicity", count: 1 },
    delay: 500,
  },
  {
    id: "reply-1",
    type: "reply",
    content: "Sure, I'll write a Python classifier as a iii worker.",
    sendLabel: "Send",
    delay: 250,
  },
  {
    id: "code-1",
    type: "code-editor",
    filename: "content_classifier.py",
    language: "python",
    lines: [
      "from iii import register_worker, InitOptions, Logger",
      "",
      "iii = register_worker(os.environ['III_ENGINE_URL'], InitOptions(",
      "    worker_name='content-classifier',",
      "))",
      "logger = Logger()",
      "",
      "def classify(payload: dict) -> dict:",
      "    text = payload.get('text', '')",
      "    logger.info(f'Classifying content: {text[:50]}...')",
      "    score = toxicity_model.predict(text)",
      "    return { 'label': 'toxic' if score > 0.7 else 'safe', 'score': score }",
      "",
      "iii.register_function('content::classify', classify)",
    ],
    doneLabel: "Deploy",
    delay: 300,
  },
  {
    id: "status-1",
    type: "status",
    variant: "info",
    icon: "connect",
    headline: "Classifier worker connected",
    detail: "content::classify registered with the engine",
    autoAdvance: 1400,
    delay: 200,
  },

  {
    id: "msg-2",
    type: "slack-message",
    sender: ALEX,
    content: "Great. Now we need to auto-approve clean content and flag toxic content for human review.",
    autoAdvance: 1500,
    delay: 500,
  },
  {
    id: "reply-2",
    type: "reply",
    content: "I'll create a TypeScript router that calls the classifier.",
    sendLabel: "Send",
    delay: 250,
  },
  {
    id: "msg-2b",
    type: "slack-message",
    sender: ALEX,
    content: "How long is that going to take? Do we need an API for the Python worker?",
    autoAdvance: 1500,
    delay: 500,
  },
  {
    id: "reply-2b",
    type: "reply",
    content: "No, it's already connected since we're using iii.",
    sendLabel: "Send",
    delay: 250,
  },
  {
    id: "code-2",
    type: "code-editor",
    filename: "content-router.ts",
    language: "typescript",
    lines: [
      "import { registerWorker } from 'iii-sdk'",
      "",
      "const iii = registerWorker(process.env.III_ENGINE_URL, {",
      "  workerName: 'content-router',",
      "})",
      "",
      "iii.registerFunction('content::route', async ({ text, author_id }) => {",
      "  const { label, score } = await iii.trigger({",
      "    function_id: 'content::classify',",
      "    payload: { text },",
      "  })",
      "  const topic = label === 'safe' ? 'comment.publish' : 'comment.review'",
      "  iii.trigger({",
      "    function_id: 'iii::durable::publish',",
      "    payload: { topic, data: { text, author_id, label, score } },",
      "  })",
      "})",
    ],
    doneLabel: "Deploy",
    delay: 300,
  },
  {
    id: "status-2",
    type: "status",
    variant: "info",
    icon: "connect",
    headline: "Content router live",
    detail: "Classifying cross-language, publishing to comment.publish and comment.review",
    autoAdvance: 1400,
    delay: 200,
  },

  {
    id: "msg-3",
    type: "slack-message",
    sender: ALEX,
    content: "Can we track how many times each author gets flagged? We need moderation history.",
    autoAdvance: 1500,
    delay: 500,
  },
  { id: "reply-3", type: "reply", content: "I'll add state.", sendLabel: "Send", delay: 250 },
  {
    id: "terminal-3a",
    type: "terminal-command",
    command: "iii trigger compose::add worker=state",
    output: "✓ state ready (2.1s)",
    delay: 400,
  },
  {
    id: "msg-3b",
    type: "slack-message",
    sender: ALEX,
    content: "How long to roll that out?",
    autoAdvance: 1500,
    delay: 500,
  },
  {
    id: "reply-3b",
    type: "reply",
    content: "It's done, adding the author history tracking now.",
    sendLabel: "Send",
    delay: 250,
  },
  {
    id: "code-3",
    type: "code-editor",
    filename: "content_classifier.py",
    language: "python",
    lines: [
      "def on_flagged(event: dict) -> None:",
      "    author_id = event.get('author_id')",
      "    flags = iii.trigger({ 'function_id': 'state::get',",
      "        'payload': { 'scope': 'moderation', 'key': f'flags:{author_id}' } })",
      "    iii.trigger({ 'function_id': 'state::set',",
      "        'payload': { 'scope': 'moderation', 'key': f'flags:{author_id}',",
      "                     'value': (flags or 0) + 1 } })",
      "",
      "iii.register_function('content::on-flagged', on_flagged)",
      "",
      "iii.register_trigger({",
      "    'type': 'durable:subscriber',",
      "    'function_id': 'content::on-flagged',",
      "    'config': { 'topic': 'comment.review' },",
      "})",
    ],
    doneLabel: "Deploy",
    delay: 300,
  },
  {
    id: "status-3",
    type: "status",
    variant: "info",
    icon: "connect",
    headline: "State active",
    detail: "Per-author flag counts persisted on every comment.review event",
    autoAdvance: 1400,
    delay: 200,
  },

  {
    id: "msg-4",
    type: "slack-message",
    sender: ALEX,
    content: "The frontend team needs a REST endpoint to submit content for moderation.",
    autoAdvance: 1500,
    delay: 500,
  },
  { id: "reply-4", type: "reply", content: "No problem, we can add HTTP.", sendLabel: "Send", delay: 250 },
  {
    id: "terminal-4a",
    type: "terminal-command",
    command: "iii trigger compose::add worker=http",
    output: "✓ http ready (2.1s)",
    delay: 400,
  },
  {
    id: "code-4",
    type: "code-editor",
    filename: "content-router.ts",
    language: "typescript",
    lines: [
      "iii.registerFunction(",
      "  'http::submit-content',",
      "  async (payload: { body: { text: string; author_id: string } }) => {",
      "    const result = await iii.trigger({",
      "      function_id: 'content::route',",
      "      payload: payload.body,",
      "    })",
      "    return {",
      "      status_code: 200,",
      "      body: result,",
      "      headers: { 'Content-Type': 'application/json' },",
      "    }",
      "  },",
      ")",
      "",
      "iii.registerTrigger({",
      "  type: 'http',",
      "  function_id: 'http::submit-content',",
      "  config: { api_path: '/content/submit', http_method: 'POST' },",
      "})",
    ],
    doneLabel: "Deploy",
    delay: 300,
  },
  {
    id: "status-4",
    type: "status",
    variant: "info",
    icon: "connect",
    headline: "POST /content/submit",
    detail: "HTTP endpoint live",
    autoAdvance: 1400,
    delay: 200,
  },

  {
    id: "msg-5",
    type: "slack-message",
    sender: ALEX,
    content: "Let me guess, it's already done?",
    autoAdvance: 1500,
    delay: 600,
  },
  {
    id: "reply-5",
    type: "reply",
    content: "Yup! It's a single system. Everything is already connected.",
    sendLabel: "Send",
    delay: 250,
  },
  {
    id: "status-5",
    type: "status",
    variant: "success",
    icon: "check",
    headline: "All services connected",
    detail: "Every worker shares the same event bus and state",
    autoAdvance: 1600,
    delay: 200,
  },

  {
    id: "msg-6",
    type: "slack-message",
    sender: ALEX,
    content: "We're seeing some classification requests fail intermittently. Can you investigate?",
    autoAdvance: 1500,
    delay: 500,
  },
  {
    id: "reply-6",
    type: "reply",
    content: "Checking the traces now, should only take a moment.",
    sendLabel: "Send",
    delay: 250,
  },
  {
    id: "console-1",
    type: "console-trace",
    traces: [
      { operation: "content::route", duration: "2.4ms", status: "ok" },
      { operation: "content::classify", duration: "1.1ms", status: "ok" },
      { operation: "content::classify", status: "error" },
    ],
    activeTraceIndex: 2,
    spans: [
      { label: "content::route", duration: "6.22ms", widthPercent: 100, depth: 0, status: "ok" },
      { label: "content::classify", duration: "4.99ms", widthPercent: 80, depth: 1, status: "ok" },
      { label: "detoxify.predict", duration: "0.3ms", widthPercent: 8, depth: 2, status: "error" },
    ],
    errorSpanIndex: 2,
    detail: {
      status: "ERROR",
      service: "content-classifier",
      error: "detoxify 0.5.2 — RuntimeError: index out of range in self",
    },
    delay: 400,
  },
  {
    id: "reply-7",
    type: "reply",
    content: "Found it — known bug in detoxify 0.5.2. Bumping to 0.5.3 and restarting the worker.",
    sendLabel: "Send",
    delay: 250,
  },
  {
    id: "status-6a",
    type: "status",
    variant: "info",
    icon: "deploy",
    headline: "detoxify==0.5.2 → detoxify==0.5.3",
    detail: "requirements.txt updated",
    autoAdvance: 1000,
    delay: 400,
  },
  {
    id: "terminal-6b",
    type: "terminal-command",
    command: "iii trigger compose::restart worker=content-classifier",
    output: "✓ content-classifier ready (1.8s)",
    delay: 300,
  },
  {
    id: "status-6",
    type: "status",
    variant: "success",
    icon: "observe",
    headline: "Issue resolved",
    detail: "Classifier restarted with detoxify==0.5.3",
    autoAdvance: 1600,
    delay: 200,
  },
]

export const HOMEPAGE_FLOW: readonly Step[] = RAW_FLOW.map(scaleTimings)
