// The worker catalog behind the landing page's "any service, one abstraction"
// ticker: the four workers from the iii registry (workers.iii.dev). Names,
// versions, descriptions, install commands, function ids and counts are
// taken from each worker's registry page; nothing here is invented.

export type WorkerName = "storage" | "database" | "cron" | "queue"

export type Worker = {
  name: WorkerName
  /** registry version at the time of writing */
  version: string
  /** the registry's one-line description */
  desc: string
  /** what a person types into the registry search to find it (one of its real tags) */
  query: string
  /** the card's `›` line: what the worker registers, then its size */
  registers: string
  functions: number
  triggers: number
  /** every worker in the registry is a binary; "verified" is the registry's own badge */
  verified: boolean
  url: string
}

const REGISTRY = "https://workers.iii.dev/workers"

export const WORKERS: readonly Worker[] = [
  {
    name: "storage",
    version: "v0.1.23",
    desc: "Object storage across AWS S3, GCS, Cloudflare R2, and a native local filesystem backend. Streamed uploads, signed URLs, and object change triggers.",
    query: "object storage",
    registers: "storage::{put, get, presign}",
    functions: 10,
    triggers: 2,
    verified: true,
    url: `${REGISTRY}/storage`,
  },
  {
    name: "database",
    version: "v0.5.20",
    desc: "Talk to PostgreSQL, MySQL, and SQLite from iii: query, execute, transactions, prepared statements, and change feeds.",
    query: "postgres",
    registers: "database::{query, execute, save}",
    functions: 30,
    triggers: 1,
    verified: true,
    url: `${REGISTRY}/database`,
  },
  {
    name: "cron",
    version: "v0.21.25",
    desc: "Schedule functions with cron expressions. Registers the cron trigger type.",
    query: "schedule",
    registers: 'trigger type "cron" · jobs::tick',
    functions: 2,
    triggers: 1,
    verified: true,
    url: `${REGISTRY}/cron`,
  },
  {
    name: "queue",
    version: "v0.21.17",
    desc: "Durable function queues. Registers the durable:subscriber trigger type and the queue/DLQ service functions.",
    query: "jobs",
    registers: 'trigger type "durable:subscriber"',
    functions: 1,
    triggers: 1,
    verified: true,
    url: `${REGISTRY}/queue`,
  },
]

/** `n` distinct workers other than `target`, in a random order (for the cards between two matches). */
export function randomWorkersExcept(target: Worker, n: number) {
  const pool = shuffle(WORKERS.filter((w) => w.name !== target.name))
  return pool.slice(0, Math.min(n, pool.length))
}

function shuffle<T>(list: readonly T[]) {
  const arr = [...list]
  for (let i = arr.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    ;[arr[i], arr[j]] = [arr[j], arr[i]]
  }
  return arr
}

/** The catalog in a random order: one full pass of the ticker. */
export function buildPlayOrder(): Worker[] {
  return shuffle(WORKERS)
}

/** Next pass, never opening on the worker the last pass closed with. */
export function nextPlayOrder(previous: readonly Worker[]) {
  const last = previous[previous.length - 1]?.name
  let order = buildPlayOrder()
  for (let attempt = 1; order[0].name === last && attempt < 10; attempt++) order = buildPlayOrder()
  return order
}

/** The `$` line typed into a card: the registry's install command. */
export function addCommand(w: Worker) {
  return `iii trigger compose::add worker=${w.name}`
}

/** The `›` line typed into a card: what the worker registers, then how big it is. */
export function registersLine(w: Worker) {
  const fns = `${w.functions} ${w.functions === 1 ? "function" : "functions"}`
  const trg = `${w.triggers} ${w.triggers === 1 ? "trigger" : "triggers"}`
  return `${w.registers}\n    ${fns} · ${trg}`
}
