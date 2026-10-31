// Derived from sdk/packages/node/iii/src/observer-handle.ts by scripts/sync-browser-observe.py; edit those, not this file.
/**
 * The observing side of observe (sdk.md §2): the handle `iii.observe`
 * returns, kept for replay until it is disposed.
 */
import { ObserveRejection } from './errors'
import type { ObserveMessage } from './iii-types'
import type { ObserveReason, ObserverHandle, ObserverState } from './observe-types'

type Listener = (state: ObserverState, reason?: ObserveReason) => void

/** Active → resolved; error / disposed → rejected; otherwise still waiting. */
function settlement(id: string, state: ObserverState, reason: ObserveReason | undefined): Promise<void> | undefined {
  if (state === 'active') return Promise.resolve()
  if (state === 'error' || state === 'disposed') {
    const why = reason ?? {
      code: state === 'error' ? 'OBSERVE_FAILED' : 'UNOBSERVED',
      message: `observer ${id} is ${state}`,
    }
    return Promise.reject(new ObserveRejection(why.code, why.message))
  }
  return undefined
}

export class ObserverHandleImpl implements ObserverHandle {
  private currentState: ObserverState = 'pending'
  private currentReason?: ObserveReason
  private readonly listeners = new Set<Listener>()
  /** State changes not delivered to the listeners yet, in order. */
  private readonly undelivered: [ObserverState, ObserveReason | undefined][] = []
  private delivering = false
  /** @internal Sent on some connection: the engine may hold it, so disposing it needs an `unobserve`. */
  sent = false
  /** @internal The engine's `observeresult` for its `observe` on this connection arrived (decision 56). */
  answered = false

  constructor(
    readonly id: string,
    /** @internal What `observe` sends, replayed on every connection. */
    readonly message: Omit<ObserveMessage, 'message_type'>,
    private readonly onDispose: (handle: ObserverHandleImpl) => void,
    private readonly logError: (message: string, error?: unknown) => void,
  ) {}

  get state(): ObserverState {
    return this.currentState
  }

  get reason(): ObserveReason | undefined {
    return this.currentReason
  }

  ready(): Promise<void> {
    const settled = settlement(this.id, this.currentState, this.currentReason)
    if (settled) return settled
    return new Promise<void>((resolve, reject) => {
      const off = this.onStateChange((state, reason) => {
        const result = settlement(this.id, state, reason)
        if (!result) return
        off()
        result.then(resolve, reject)
      })
    })
  }

  onStateChange(listener: Listener): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  async dispose(): Promise<void> {
    if (this.currentState === 'disposed') return
    this.onDispose(this)
    this.setState('disposed', { code: 'UNOBSERVED', message: 'disposed by the observing worker' })
  }

  /** @internal A state from the engine (`observeresult`, `observerstatechanged`) or the SDK. */
  setState(state: ObserverState, reason?: ObserveReason): void {
    if (this.currentState === 'disposed') return
    const changed =
      state !== this.currentState ||
      reason?.code !== this.currentReason?.code ||
      reason?.message !== this.currentReason?.message
    this.currentState = state
    this.currentReason = reason
    if (!changed) return
    // Delivered later, in order: a listener never runs on the socket's read
    // path (or inside dispose()), so it may call the client freely.
    this.undelivered.push([state, reason])
    if (this.delivering) return
    this.delivering = true
    queueMicrotask(() => this.deliver())
  }

  private deliver(): void {
    for (let next = this.undelivered.shift(); next; next = this.undelivered.shift()) {
      const [state, reason] = next
      for (const listener of [...this.listeners]) {
        try {
          listener(state, reason)
        } catch (error) {
          this.logError(`Observer ${this.id} state listener threw`, error)
        }
      }
    }
    this.delivering = false
  }
}

const STATES: ReadonlySet<string> = new Set(['pending', 'active', 'suspended', 'error', 'disposed'])

/** A wire state this SDK does not know (a newer engine) reads as `pending`. */
export function toObserverState(state: string): ObserverState {
  return (STATES.has(state) ? state : 'pending') as ObserverState
}
