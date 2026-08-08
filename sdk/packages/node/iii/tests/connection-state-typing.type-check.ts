/**
 * Type-level regression test: the PUBLIC `IIIClient` returned by
 * `registerWorker()` must expose `getConnectionState()` and `getFatalError()`
 * (SDK parity with the Python/Rust SDKs). These live on the concrete class, so
 * without declaring them on the `IIIClient` interface a normal consumer gets
 * `TS2339: Property 'getConnectionState' does not exist on type 'IIIClient'`.
 * Not executed at runtime — vitest only picks up `*.test.ts` — but `tsc
 * --noEmit` (run in CI) compiles it.
 */
import { type IIIConnectionState, RegistrationRejectedError, registerWorker } from '../src/index'

// biome-ignore lint/correctness/noUnusedVariables: compile-only assertions
function connectionStateTypingAssertions() {
  const worker = registerWorker('ws://localhost:49134')

  const state: IIIConnectionState = worker.getConnectionState()
  void state

  const fatal: RegistrationRejectedError | undefined = worker.getFatalError()
  if (fatal) {
    // The exported error type carries the structured rejection fields.
    const code: string = fatal.code
    const namespace: string = fatal.namespace
    void code
    void namespace
  }
}
