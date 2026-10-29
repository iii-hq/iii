/**
 * Recipe 2 of 3: queryable records with live updates.
 *
 * Replaces: the iii-stream `set` / `get` / `list` functions as a generic KV store, plus
 * observing iii-stream changes (or the Streams WebSocket) to watch it.
 *
 * With: explicit storage owned by the domain worker, `tasks::get` and a paginated
 * `tasks::list`, and a private `tasks::on-changed` source that the worker calls ONLY AFTER
 * the write is committed, with `{ id, revision, op }` (notify, then query). The consumer:
 *   1. observes first, then does its initial read (paginated list);
 *   2. on each notification re-reads the record unless it already has that revision, so
 *      duplicates and out-of-order deliveries are harmless;
 *   3. after a reconnect (or restart), re-queries everything (resync), because
 *      notifications fired while it was away are gone: the store is the source of truth.
 *
 * Run against an engine (iii-stream NOT required):
 *   III_URL=ws://127.0.0.1:49134 node src/streams-migration/stored-records.ts
 *
 * Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams
 */
import { type IIIClient, ObserveRejection, registerWorker } from 'iii-sdk'

const ENGINE_URL = process.env.III_URL ?? 'ws://127.0.0.1:49134'
const RUN_ID = Math.random().toString(36).slice(2, 8)

export type Task = {
  id: string
  title: string
  done: boolean
  revision: number
  updated_at: number
}
export type TaskChange = { id: string; revision: number; op: 'upserted' | 'deleted' }
/** An observer's args: watch one task, or every task without `id`. */
export type TaskChangedArgs = { id?: string }
type GetResult = { task: Task | null; revision: number }
type ListResult = { tasks: Task[]; next_cursor?: string }

export const TASK_CHANGED = 'tasks::on-changed'
const MAX_OBSERVERS = 256
const MAX_PAGE = 100

// ── Storage ────────────────────────────────────────────────────────────────────────

/**
 * DEMO STORAGE: an in-memory Map, so this recipe runs on a bare engine and loses its data
 * when the process exits. In production this is your source of truth: a database, or the
 * `state` worker (`state::set` / `state::get` / `state::list` through `iii.call`).
 * What matters is the contract: a write returns only once it is committed, every write
 * bumps a per-record revision, and deletes keep a tombstone revision so that revisions
 * never go backwards and a late event can never resurrect a deleted record.
 */
function createDemoTaskStore() {
  const rows = new Map<string, Task>()
  const tombstones = new Map<string, number>()
  const nextRevision = (id: string) => (rows.get(id)?.revision ?? tombstones.get(id) ?? 0) + 1

  return {
    get(id: string): GetResult {
      const task = rows.get(id) ?? null
      return { task, revision: task?.revision ?? tombstones.get(id) ?? 0 }
    },
    /** Ordered by id; `cursor` is the last id of the previous page. */
    list(limit: number, cursor?: string): ListResult {
      const ids = [...rows.keys()].sort().filter(id => cursor === undefined || id > cursor)
      const page = ids.slice(0, limit)
      const tasks = page.map(id => rows.get(id) as Task)
      return ids.length > limit ? { tasks, next_cursor: page[page.length - 1] } : { tasks }
    },
    upsert(input: { id: string; title?: string; done?: boolean }): Task {
      const previous = rows.get(input.id)
      const task: Task = {
        id: input.id,
        title: input.title ?? previous?.title ?? '',
        done: input.done ?? previous?.done ?? false,
        revision: nextRevision(input.id),
        updated_at: Date.now(),
      }
      rows.set(task.id, task)
      tombstones.delete(task.id)
      return task
    },
    delete(id: string): number | null {
      if (!rows.has(id)) return null
      const revision = nextRevision(id)
      rows.delete(id)
      tombstones.set(id, revision)
      return revision
    },
    snapshot: () => new Map([...rows].map(([id, task]) => [id, task.revision])),
  }
}

// ── Provider: the tasks worker owns storage, queries and the change source ────

export function startTasksWorker(engineUrl = ENGINE_URL) {
  const iii = registerWorker(engineUrl, { workerName: `tasks-${RUN_ID}`, otel: { enabled: false } })
  const store = createDemoTaskStore()

  // The source: the handler returns the change it is called with, which is what observers
  // observe. `private`: only this worker calls it. The engine validates observers' args
  // against `args_format` (INVALID_ARGS); matching by keys reaches the observers of one task
  // and those of every task, with one lookup per key.
  const changed = iii
    .registerFunction(TASK_CHANGED, (change: TaskChange) => change, {
      description:
        'Called after a task write is committed: { id, revision, op }. Observe it with args { id? }. ' +
        'Notification only: read the record with tasks::get / tasks::list.',
      args_format: {
        type: 'object',
        additionalProperties: false,
        properties: { id: { type: 'string', minLength: 1 } },
      },
      private: true,
    })
    .onObserverAdded(obs => {
      if (changed.observers().length >= MAX_OBSERVERS) {
        throw new ObserveRejection(
          'OBSERVER_LIMIT',
          `${TASK_CHANGED}: observer limit (${MAX_OBSERVERS}) reached`,
        )
      }
      console.log(`[tasks] observed by ${obs.id} -> ${obs.target}`)
    })
    .onObserverRemoved(obs => {
      console.log(`[tasks] observer ${obs.id} removed`)
    })
    .onObserverMatch({
      observer: obs => (obs.args as TaskChangedArgs).id ?? '*',
      call: call => [call.input.id, '*'],
    })

  /** Notify: one call of the source; iii delivers it to the matching observers. */
  async function emit(change: TaskChange): Promise<void> {
    if (!changed.isObserved({ input: change })) return
    await iii.call({ function_id: TASK_CHANGED, payload: change })
  }

  iii.registerFunction(
    'tasks::upsert',
    async (input: { id: string; title?: string; done?: boolean }) => {
      const task = store.upsert(input) // 1. commit (a failed write throws here: nothing is emitted)
      await emit({ id: task.id, revision: task.revision, op: 'upserted' }) // 2. then notify
      return task
    },
    {
      description: 'Create or update a task, then notify tasks::on-changed',
      request_format: {
        type: 'object',
        required: ['id'],
        properties: {
          id: { type: 'string' },
          title: { type: 'string' },
          done: { type: 'boolean' },
        },
      },
    },
  )

  iii.registerFunction(
    'tasks::delete',
    async (input: { id: string }) => {
      const revision = store.delete(input.id)
      if (revision === null) return { deleted: false }
      await emit({ id: input.id, revision, op: 'deleted' })
      return { deleted: true, revision }
    },
    { description: 'Delete a task, then notify tasks::on-changed' },
  )

  // Demo-only failure injection, used by the self-check at the end of main().
  let failReads = 0
  const maybeFail = () => {
    if (failReads > 0) {
      failReads--
      throw new Error('injected read failure (demo)')
    }
  }

  iii.registerFunction(
    'tasks::get',
    async (input: { id: string }) => {
      maybeFail()
      return store.get(input.id)
    },
    {
      description:
        'Read one task: { task | null, revision } (revision of the tombstone when deleted)',
    },
  )

  iii.registerFunction(
    'tasks::list',
    async (input: { limit?: number; cursor?: string }) => {
      maybeFail()
      return store.list(Math.min(Math.max(input?.limit ?? 50, 1), MAX_PAGE), input?.cursor)
    },
    {
      description: `List tasks ordered by id; limit (default 50, max ${MAX_PAGE}) + opaque cursor`,
    },
  )

  return {
    iii,
    store,
    observerCount: () => changed.observers().length,
    /** Demo only: make the next tasks::get / tasks::list call fail. */
    failNextRead: () => {
      failReads = 1
    },
  }
}

// ── Consumer: a dashboard keeping a local view in sync ─────────────────────────

/** Local view: highest revision seen per id; `task: null` is a tombstone. */
export type TaskView = Map<string, { revision: number; task: Task | null }>

const MAX_DIRTY = 1000 // bound on pending re-reads; beyond it the consumer falls back to one resync
const PAGE_SIZE = 2 // tiny on purpose, so the demo shows pagination
const MAX_RETRY_DELAY_MS = 30_000

export function startDashboard(name: string, view: TaskView = new Map(), engineUrl = ENGINE_URL) {
  const iii = registerWorker(engineUrl, {
    workerName: `dashboard-${name}-${RUN_ID}`,
    otel: { enabled: false },
  })
  const handlerId = `dashboard::on-task-changed::${name}`
  const stats = { applied: 0, skipped: 0, reads: 0, resyncs: 0 }
  const dirty = new Set<string>()
  let needsResync = false
  let draining: Promise<void> | null = null
  let retryDelayMs = 0
  let retryTimer: ReturnType<typeof setTimeout> | undefined

  /** Keep only newer revisions: this is what makes duplicates and reordering harmless. */
  function apply(id: string, revision: number, task: Task | null): void {
    const current = view.get(id)
    if (current && current.revision >= revision) return
    view.set(id, { revision, task })
    stats.applied++
  }

  /** Initial read and recovery: page through the owner's list, then settle local ids it no longer returns. */
  async function resync(): Promise<void> {
    stats.resyncs++
    const seen = new Set<string>()
    let cursor: string | undefined
    let pages = 0
    do {
      const page = await iii.call<unknown, ListResult>({
        function_id: 'tasks::list',
        payload: { limit: PAGE_SIZE, ...(cursor ? { cursor } : {}) },
      })
      pages++
      for (const task of page.tasks) {
        seen.add(task.id)
        apply(task.id, task.revision, task)
      }
      cursor = page.next_cursor
    } while (cursor)
    // Deleted while we were not listening: ask for the tombstone revision instead of guessing.
    for (const [id, entry] of view) {
      if (entry.task !== null && !seen.has(id)) await readOne(id)
    }
    console.log(`[dashboard ${name}] resync: ${pages} page(s), ${seen.size} live task(s)`)
  }

  async function readOne(id: string): Promise<void> {
    stats.reads++
    const result = await iii.call<unknown, GetResult>({
      function_id: 'tasks::get',
      payload: { id },
    })
    apply(id, result.revision, result.task)
  }

  /** Single drain loop: at most one read in flight, a bounded dirty set, no task per event. */
  function drain(): Promise<void> {
    if (draining) return draining
    // Nothing to do: never create (and never cache) an empty run.
    if (!needsResync && dirty.size === 0) return Promise.resolve()
    let run: Promise<void> | undefined
    run = (async () => {
      // Start asynchronously so `draining` is assigned before this run can finish.
      await Promise.resolve()
      try {
        while (needsResync || dirty.size > 0) {
          if (needsResync) {
            needsResync = false
            dirty.clear()
            await resync()
            continue
          }
          const id = dirty.values().next().value as string
          dirty.delete(id)
          await readOne(id)
        }
        // Caught up: a retry scheduled by an earlier failure is no longer needed.
        clearTimeout(retryTimer)
        retryTimer = undefined
        retryDelayMs = 0
      } catch (error) {
        // The owner may be unreachable (e.g. it has not re-registered yet after an engine
        // restart). Fall back to ONE full resync, retried with capped backoff: no spinning,
        // no queue of failed reads.
        needsResync = true
        retryDelayMs = Math.min(retryDelayMs === 0 ? 500 : retryDelayMs * 2, MAX_RETRY_DELAY_MS)
        console.warn(
          `[dashboard ${name}] read failed (${(error as Error).message}); retry in ${retryDelayMs}ms`,
        )
        clearTimeout(retryTimer)
        retryTimer = setTimeout(() => {
          retryTimer = undefined
          void drain()
        }, retryDelayMs)
        retryTimer.unref()
      } finally {
        // Only clear the slot if it still belongs to this run.
        if (draining === run) draining = null
      }
    })()
    draining = run
    return run
  }

  iii.registerFunction(handlerId, async (change: TaskChange) => {
    const known = view.get(change.id)?.revision ?? 0
    if (change.revision <= known) {
      stats.skipped++ // duplicate or out-of-order delivery
      console.log(`[dashboard ${name}] skip ${change.id} r${change.revision} (have r${known})`)
      return null
    }
    if (dirty.size >= MAX_DIRTY) needsResync = true
    else dirty.add(change.id)
    void drain()
    return null
  })

  // 1. Observe first, so nothing committed after the read below can be missed ...
  const observer = iii.observe({ function_id: TASK_CHANGED, deliver: 'input' }, handlerId)

  // 3. ... and re-query after every reconnect: notifications fired while disconnected are gone.
  // (getConnectionState() is a local read; the SDK replays the observer by itself on reconnect.)
  let watching = false
  let wasConnected = false
  const reconnectWatch = setInterval(() => {
    const connected = iii.getConnectionState() === 'connected'
    if (watching && connected && !wasConnected) {
      console.log(`[dashboard ${name}] reconnected: resync`)
      needsResync = true
      void drain()
    }
    wasConnected = connected
  }, 1000)
  reconnectWatch.unref()

  return {
    iii,
    view,
    stats,
    observer,
    /** 2. Initial read (call once the observer is active). Also the recovery path. */
    async initialRead() {
      await observer.ready()
      needsResync = true
      await drain()
      wasConnected = iii.getConnectionState() === 'connected'
      watching = true
    },
    idle: () => draining === null && dirty.size === 0 && !needsResync,
    /** Demo introspection: a retry of a failed read is scheduled. */
    retryPending: () => retryTimer !== undefined,
    /** Leaves: the observer is disposed (the provider's onObserverRemoved runs), then the worker stops. */
    async stop() {
      clearInterval(reconnectWatch)
      clearTimeout(retryTimer)
      await observer.dispose()
      await iii.shutdown()
    },
  }
}

// ── Demo ─────────────────────────────────────────────────────────────────────────

const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

async function waitFor(label: string, condition: () => boolean, timeoutMs = 5000): Promise<void> {
  const start = Date.now()
  while (!condition()) {
    if (Date.now() - start > timeoutMs) throw new Error(`timed out waiting for: ${label}`)
    await sleep(25)
  }
}

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(`check failed: ${message}`)
  console.log(`[demo] ok: ${message}`)
}

function liveRevisions(view: TaskView): string {
  return [...view]
    .filter(([, entry]) => entry.task !== null)
    .map(([id, entry]) => `${id}@r${entry.revision}`)
    .sort()
    .join(',')
}

async function untilCallable(caller: IIIClient, function_id: string, payload: unknown) {
  const start = Date.now()
  for (;;) {
    try {
      return await caller.call({ function_id, payload })
    } catch (error) {
      const notYet = String((error as Error).message).includes('function_not_found')
      if (!notYet || Date.now() - start > 5000) throw error
      await sleep(50)
    }
  }
}

const upsert = (caller: IIIClient, payload: { id: string; title?: string; done?: boolean }) =>
  caller.call<unknown, Task>({ function_id: 'tasks::upsert', payload })

async function main(): Promise<void> {
  console.log(`[demo] engine ${ENGINE_URL}`)
  const tasks = startTasksWorker()
  let dashboard: ReturnType<typeof startDashboard> | undefined

  try {
    // Registration is asynchronous: wait until the provider's functions are callable.
    await untilCallable(tasks.iii, 'tasks::list', { limit: 1 })

    // Records committed before any consumer exists: only the initial read can show them.
    for (const id of ['task-a', 'task-b', 'task-c'])
      await upsert(tasks.iii, { id, title: `Title of ${id}` })

    dashboard = startDashboard('one')
    await waitFor('observer active', () => tasks.observerCount() === 1)
    await dashboard.initialRead()
    check(
      liveRevisions(dashboard.view) === 'task-a@r1,task-b@r1,task-c@r1',
      'initial read loaded 3 tasks over 2 pages',
    )

    // Live: notification -> re-read.
    await upsert(tasks.iii, { id: 'task-a', done: true })
    await waitFor('task-a r2', () => dashboard?.view.get('task-a')?.revision === 2)
    check(
      dashboard.view.get('task-a')?.task?.done === true,
      'live update applied (task-a r2, done)',
    )

    // At-least-once delivery: simulate a duplicate and a late (out-of-order) redelivery.
    const before = dashboard.stats.skipped
    for (const revision of [2, 1]) {
      await tasks.iii.call({
        function_id: 'dashboard::on-task-changed::one',
        payload: { id: 'task-a', revision, op: 'upserted' } satisfies TaskChange,
      })
    }
    check(
      dashboard.stats.skipped - before === 2,
      'duplicate r2 and stale r1 were skipped by revision',
    )
    check(dashboard.view.get('task-a')?.revision === 2, 'view still at task-a r2')

    // The consumer goes away: its observer is removed at the provider.
    const keptView = dashboard.view
    await dashboard.stop()
    await waitFor('provider saw the consumer leave', () => tasks.observerCount() === 0)

    // Writes while nobody is listening: these notifications are delivered to no one.
    await upsert(tasks.iii, { id: 'task-b', title: 'Renamed while offline' })
    await tasks.iii.call({ function_id: 'tasks::delete', payload: { id: 'task-c' } })
    await upsert(tasks.iii, { id: 'task-d', title: 'Created while offline' })
    check(
      liveRevisions(keptView) === 'task-a@r2,task-b@r1,task-c@r1',
      'cached view is stale after the outage',
    )

    // Reconnect with the cached view: observe again, then resync by re-querying the owner.
    dashboard = startDashboard('two', keptView)
    await waitFor('observer active again', () => tasks.observerCount() === 1)
    await dashboard.initialRead()
    const expected = [...tasks.store.snapshot()]
      .map(([id, rev]) => `${id}@r${rev}`)
      .sort()
      .join(',')
    check(
      liveRevisions(dashboard.view) === expected,
      `recovered view matches the store (${expected})`,
    )
    check(dashboard.view.get('task-c')?.task === null, 'task-c tombstoned at its delete revision')

    // And live updates flow again after recovery.
    await upsert(tasks.iii, { id: 'task-d', done: true })
    await waitFor('task-d r2', () => dashboard?.view.get('task-d')?.revision === 2)
    check(
      dashboard.view.get('task-d')?.task?.done === true,
      'live update after recovery (task-d r2)',
    )

    // Self-check (review fix): a read fails and schedules a retry; a notification completes the
    // resync before the retry timer fires; then the timer fires with nothing left to do. The
    // consumer must stay live: later changes are applied and it goes idle.
    tasks.failNextRead()
    await upsert(tasks.iii, { id: 'task-e', title: 'retry path' }) // r1: tasks::get fails
    await waitFor('retry scheduled after the failed read', () => dashboard?.retryPending() === true)
    await upsert(tasks.iii, { id: 'task-e', done: true }) // r2: the notification drives the resync
    await waitFor('task-e r2 via resync', () => dashboard?.view.get('task-e')?.revision === 2)
    await sleep(800) // past the 500 ms retry delay
    await upsert(tasks.iii, { id: 'task-e', title: 'after the retry timer' }) // r3
    await waitFor(
      'task-e r3 applied after the retry timer',
      () => dashboard?.view.get('task-e')?.revision === 3,
      3000,
    )
    await waitFor('consumer idle', () => dashboard?.idle() === true, 3000)
    check(true, 'change after a stale retry timer was applied and the consumer went idle')
    check(!dashboard.retryPending(), 'no retry left pending after a successful resync')

    console.log(`[demo] stats ${JSON.stringify(dashboard.stats)}`)
    console.log('[demo] stored-records: PASS')
  } finally {
    await dashboard?.stop()
    await tasks.iii.shutdown()
  }
}

main().then(
  () => process.exit(0),
  error => {
    console.error('[demo] stored-records: FAIL', error)
    process.exit(1)
  },
)
