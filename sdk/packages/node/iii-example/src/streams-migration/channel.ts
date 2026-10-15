/**
 * Recipe 3 of 3: continuous content for ONE operation.
 *
 * Replaces: writing progress / partial output into stream items (an iii-stream `set` per
 * chunk, one group per job) and having the caller watch them through the deprecated `stream`
 * trigger type or the dedicated Streams WebSocket.
 *
 * With: a channel created for the operation (`createChannel`) whose writer ref travels in
 * the call. The lifetime is explicit: the channel exists for this operation only, the
 * producer closes it when done (or at its deadline), and the caller enforces its own
 * deadline and can stop early with `reports::cancel` plus closing its end. The producer
 * caps concurrent operations. Buffering is bounded end to end: the engine buffer (`bufferSize`),
 * the producer awaiting 'drain' (Node Writable backpressure), the reader pausing the
 * socket when its Readable is full, and a cap on a single NDJSON line on the caller.
 *
 * Run against an engine (iii-stream NOT required):
 *   III_URL=ws://127.0.0.1:49134 node src/streams-migration/channel.ts
 *
 * Migration guide: https://iii.dev/docs/upgrading/migrate-from-streams
 */
import { once } from 'node:events'
import { Writable } from 'node:stream'
import { type IIIClient, registerWorker, TriggerAction } from 'iii-sdk'
import type { StreamChannelRef } from 'iii-sdk/channel'
import { createChannel } from 'iii-sdk/helpers'

const ENGINE_URL = process.env.III_URL ?? 'ws://127.0.0.1:49134'
const RUN_ID = Math.random().toString(36).slice(2, 8)

/** Engine-side channel buffer (passed to engine::channels::create). */
const CHANNEL_BUFFER = 16
/** Hard cap on one NDJSON line held by the reader; a longer line aborts the operation. */
const MAX_LINE_BYTES = 64 * 1024
/** Concurrent operations the producer accepts; more are rejected, never queued. */
const MAX_ACTIVE_OPERATIONS = 32

export type ExportLine =
  | { type: 'header'; report_id: string; rows: number }
  | { type: 'row'; n: number; value: string }
  | { type: 'end'; rows_written: number }

type ExportRequest<TWriter> = {
  operation_id: string
  report_id: string
  rows: number
  deadline_ms: number
  writer: TWriter
}
/** What the handler uses of the incoming ChannelWriter: its Writable (a plain holder works too). */
type Writer = { stream: Writable }
type ExportResult = {
  rows_written: number
  aborted: boolean
  reason?: string
  /** Largest local write buffer observed; stays near the Writable high-water mark thanks to 'drain'. */
  max_buffered_bytes: number
  high_water_mark: number
}

// ── Producer: the reports worker writes the content into the caller's channel ──

export function startReportsWorker(engineUrl = ENGINE_URL) {
  const iii = registerWorker(engineUrl, {
    workerName: `reports-${RUN_ID}`,
    otel: { enabled: false },
  })
  /** Live operations and their cancel flag. Bounded by MAX_ACTIVE_OPERATIONS. */
  const active = new Map<string, { cancelled: boolean }>()

  // The SDK turns the incoming writer ref into a live ChannelWriter before the handler runs.
  // Only its `stream` is used, which also lets the demo self-check call the handler directly.
  const exportReport = async ({
    operation_id,
    report_id,
    rows,
    deadline_ms,
    writer,
  }: ExportRequest<Writer>): Promise<ExportResult> => {
    const out = writer.stream
    if (active.size >= MAX_ACTIVE_OPERATIONS || active.has(operation_id)) {
      out.end()
      throw new Error('reports::export: too many concurrent operations (or duplicate operation_id)')
    }
    const operation = { cancelled: false }
    active.set(operation_id, operation)
    const deadline = Date.now() + deadline_ms
    let maxBuffered = 0
    let written = 0
    const stats = () => ({
      max_buffered_bytes: maxBuffered,
      high_water_mark: out.writableHighWaterMark,
    })
    out.on('error', () => undefined) // surfaced through out.destroyed and the result instead

    /** The stream can no longer take writes (destroyed, ended or closed): never wait on it. */
    const dead = () => out.destroyed || out.writableEnded || out.closed
    /** Resolve on 'drain' (or when the stream dies), removing the listeners each time. */
    const drained = () =>
      new Promise<void>(resolve => {
        // 'close' may already have fired: resolve now instead of waiting forever.
        if (dead()) return resolve()
        const done = () => {
          out.off('drain', done)
          out.off('close', done)
          resolve()
        }
        out.on('drain', done)
        out.on('close', done)
      })
    /** Write one line; wait when the local buffer is full, so memory stays bounded. */
    const writeLine = async (line: ExportLine) => {
      if (dead()) return // the loop's stopReason() check reports why
      const ok = out.write(`${JSON.stringify(line)}\n`)
      maxBuffered = Math.max(maxBuffered, out.writableLength)
      if (!ok) await drained()
    }
    const stopReason = () =>
      operation.cancelled
        ? 'cancelled by caller'
        : out.destroyed
          ? 'channel closed'
          : Date.now() > deadline
            ? 'deadline'
            : undefined

    try {
      await writeLine({ type: 'header', report_id, rows })
      if (stopReason()) return { rows_written: 0, aborted: true, reason: stopReason(), ...stats() }
      for (let n = 0; n < rows; n++) {
        const reason = stopReason()
        if (reason) return { rows_written: written, aborted: true, reason, ...stats() }
        await writeLine({ type: 'row', n, value: `${report_id}-row-${n}-${'x'.repeat(64)}` })
        written++
        // Yield to the event loop now and then: a loop whose writes never block would
        // otherwise starve the socket and never see reports::cancel.
        if (written % 64 === 0) await new Promise(resolve => setImmediate(resolve))
      }
      // Re-check after the last row: a cancel or close during it must not write 'end'.
      const reason = stopReason()
      if (reason) return { rows_written: written, aborted: true, reason, ...stats() }
      await writeLine({ type: 'end', rows_written: written })
      return { rows_written: written, aborted: false, ...stats() }
    } catch (error) {
      // The other end went away mid-write: stop, do not retry.
      return {
        rows_written: written,
        aborted: true,
        reason: `channel error: ${(error as Error).message}`,
        ...stats(),
      }
    } finally {
      // Explicit end of life: always release the operation and close the writer.
      active.delete(operation_id)
      if (!out.destroyed) out.end()
    }
  }

  // Explicit cancel: the caller does not have to rely on the transport noticing a closed reader.
  const cancel = async ({ operation_id }: { operation_id: string }) => {
    const operation = active.get(operation_id)
    if (operation) operation.cancelled = true
    return { cancelled: operation !== undefined }
  }

  iii.registerFunction('reports::export', exportReport, {
    description: 'Stream a report as NDJSON lines into the channel writer passed by the caller',
  })
  iii.registerFunction('reports::cancel', cancel, {
    description: 'Cancel a running reports::export by operation_id',
  })

  return { iii, exportReport, cancel, activeCount: () => active.size }
}

// ── Caller: owns the channel for the duration of one operation ──────────────

export async function runExport(
  iii: IIIClient,
  opts: {
    report_id: string
    rows: number
    deadline_ms: number
    stopAfterRows?: number
    slowEveryRows?: number
  },
) {
  // The caller creates (and therefore owns) the channel; it stays connected until the end.
  const channel = await createChannel(iii, CHANNEL_BUFFER)
  const reader = channel.reader.stream
  const operation_id = `${opts.report_id}-${Math.random().toString(36).slice(2, 10)}`
  const lines: ExportLine[] = []
  let partial = ''
  let rowsSeen = 0
  let abortReason: string | undefined

  const abort = (reason: string) => {
    if (abortReason) return
    abortReason = reason
    // Tell the producer to stop, then close our end of the channel.
    void iii
      .trigger({
        function_id: 'reports::cancel',
        payload: { operation_id },
        action: TriggerAction.Void(),
      })
      .catch(() => undefined)
    channel.reader.close()
  }
  const deadline = setTimeout(() => {
    abort('deadline')
    reader.destroy()
  }, opts.deadline_ms)

  const operation = iii.trigger<ExportRequest<StreamChannelRef>, ExportResult>({
    function_id: 'reports::export',
    payload: {
      operation_id,
      report_id: opts.report_id,
      rows: opts.rows,
      deadline_ms: opts.deadline_ms,
      writer: channel.writerRef,
    },
    timeoutMs: opts.deadline_ms + 1000,
  })

  try {
    for await (const chunk of reader) {
      partial += (chunk as Buffer).toString('utf-8')
      let newline = partial.indexOf('\n')
      while (newline >= 0) {
        const line = JSON.parse(partial.slice(0, newline)) as ExportLine
        partial = partial.slice(newline + 1)
        lines.push(line)
        if (line.type === 'row') rowsSeen++
        newline = partial.indexOf('\n')
      }
      if (partial.length > MAX_LINE_BYTES) abort('line too long')
      if (opts.stopAfterRows !== undefined && rowsSeen >= opts.stopAfterRows)
        abort('caller stopped early')
      if (abortReason) break // leaving the loop destroys the reader stream
      // A deliberately slow reader: backpressure holds the producer instead of growing buffers.
      if (opts.slowEveryRows && rowsSeen % opts.slowEveryRows === 0)
        await new Promise(r => setTimeout(r, 5))
    }
  } catch (error) {
    if (!abortReason) throw error
  } finally {
    clearTimeout(deadline)
  }

  const result = await operation
  return { lines, rowsSeen, abortReason, result }
}

// ── Demo ─────────────────────────────────────────────────────────────────────────

async function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T | 'timeout'> {
  let timer: ReturnType<typeof setTimeout> | undefined
  const timeout = new Promise<'timeout'>(resolve => {
    timer = setTimeout(() => resolve('timeout'), ms)
  })
  try {
    return await Promise.race([promise, timeout])
  } finally {
    clearTimeout(timer)
  }
}

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(`check failed: ${message}`)
  console.log(`[demo] ok: ${message}`)
}

async function main(): Promise<void> {
  console.log(`[demo] engine ${ENGINE_URL}`)
  const reports = startReportsWorker()
  const caller = registerWorker(ENGINE_URL, {
    workerName: `report-caller-${RUN_ID}`,
    otel: { enabled: false },
  })

  try {
    // 1. Full operation with a slow reader: all content arrives in order, then the channel closes.
    const full = await runExport(caller, {
      report_id: 'r-1',
      rows: 5000,
      deadline_ms: 20_000,
      slowEveryRows: 500,
    })
    console.log(
      `[demo] full export: ${full.lines.length} lines, result ${JSON.stringify(full.result)}`,
    )
    check(full.lines[0]?.type === 'header', 'header received first')
    check(full.rowsSeen === 5000, 'all 5000 rows received')
    check(
      full.lines.every((line, i) => line.type !== 'row' || line.n === i - 1),
      'rows arrived in order',
    )
    const last = full.lines[full.lines.length - 1]
    check(
      last?.type === 'end' && last.rows_written === 5000,
      'end marker received, then the channel closed',
    )
    check(
      full.result.aborted === false && full.result.rows_written === 5000,
      'producer reported completion',
    )
    check(
      full.result.max_buffered_bytes <= full.result.high_water_mark + 1024,
      `producer buffer stayed bounded (${full.result.max_buffered_bytes} <= ${full.result.high_water_mark} + one line)`,
    )

    // 2. The caller ends the operation early by closing its end; the producer notices and stops.
    const early = await runExport(caller, {
      report_id: 'r-2',
      rows: 1_000_000,
      deadline_ms: 20_000,
      stopAfterRows: 200,
    })
    console.log(
      `[demo] early stop: saw ${early.rowsSeen} rows, result ${JSON.stringify(early.result)}`,
    )
    check(early.abortReason === 'caller stopped early', 'caller closed its end')
    check(
      early.result.aborted === true && early.result.rows_written < 100_000,
      `producer stopped early (reason: ${early.result.reason}) instead of writing 1M rows`,
    )

    // 3. Self-checks (review fix): the handler must return and release its slot even when the
    //    writer is already destroyed, or when the caller cancels and closes during the last row.
    const destroyedWriter = new Writable({ write: (_chunk, _encoding, callback) => callback() })
    destroyedWriter.destroy()
    await once(destroyedWriter, 'close')
    const onDestroyed = await withTimeout(
      reports.exportReport({
        operation_id: 'self-check-destroyed',
        report_id: 'r-3',
        rows: 10,
        deadline_ms: 5000,
        writer: { stream: destroyedWriter },
      }),
      2000,
    )
    check(onDestroyed !== 'timeout', 'handler returned for an already destroyed writer')
    check(reports.activeCount() === 0, 'active slot released (destroyed writer)')

    const lastRow = 2
    const closingWriter = new Writable({
      highWaterMark: 1, // every write reports backpressure, so the last row waits for drain/close
      write(chunk, _encoding, callback) {
        if (String(chunk).includes(`"n":${lastRow},`)) {
          void reports.cancel({ operation_id: 'self-check-last-row' })
          this.destroy() // the caller closes its end during the last row
          return
        }
        setImmediate(callback)
      },
    })
    closingWriter.on('error', () => undefined)
    const onLastRow = await withTimeout(
      reports.exportReport({
        operation_id: 'self-check-last-row',
        report_id: 'r-4',
        rows: lastRow + 1,
        deadline_ms: 5000,
        writer: { stream: closingWriter },
      }),
      2000,
    )
    check(onLastRow !== 'timeout', 'handler returned after a cancel/close during the last row')
    check(
      onLastRow !== 'timeout' && onLastRow.aborted === true,
      `last-row cancel reported as aborted (${onLastRow === 'timeout' ? 'timeout' : onLastRow.reason})`,
    )
    check(reports.activeCount() === 0, 'active slot released (last-row cancel)')

    console.log('[demo] channel: PASS')
  } finally {
    await caller.shutdown()
    await reports.iii.shutdown()
  }
}

main().then(
  () => process.exit(0),
  error => {
    console.error('[demo] channel: FAIL', error)
    process.exit(1)
  },
)
