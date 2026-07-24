/**
 * Type-level regression test: a typed browser middleware must be able to read
 * the target namespace the engine sends (engine/src/engine/mod.rs) so it can
 * re-target the forwarded call at the caller's namespace. Not executed at
 * runtime — vitest only picks up `*.test.ts` — but `tsc --noEmit` (run in CI)
 * compiles it, so a missing `namespace` field fails to type-check.
 */
import type { MiddlewareFunctionInput } from '../src/index'

// biome-ignore lint/correctness/noUnusedVariables: compile-only assertions
function middlewareInputNamespaceAssertions() {
  // Mirrors the wire object the engine builds for the RBAC middleware.
  const input = {
    function_id: 'orders::create',
    payload: {},
    context: {},
    namespace: 'orders',
  } satisfies MiddlewareFunctionInput

  // Field must be present on the type; `string | undefined` is the contract.
  const ns: string | undefined = input.namespace
  void ns

  // And it must be optional (omitting it still type-checks).
  const withoutNamespace: MiddlewareFunctionInput = {
    function_id: 'orders::create',
    payload: {},
    context: {},
  }
  void withoutNamespace
}
