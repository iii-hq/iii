export type RegistrationRejectedInit = {
  code: string
  namespace: string
  worker_name: string
  owner_worker_id: string
}

/**
 * A terminal registration rejection from the engine. Thrown into every pending
 * invocation and exposed via `getFatalError()` when a worker-name collision
 * (or other non-retryable rejection) closes the connection for good.
 */
export class RegistrationRejectedError extends Error {
  public readonly code: string
  public readonly namespace: string
  public readonly worker_name: string
  public readonly owner_worker_id: string

  constructor(init: RegistrationRejectedInit) {
    super(
      `${init.code}: registration rejected for worker "${init.worker_name}" in namespace "${init.namespace}" (already owned by worker ${init.owner_worker_id})`,
    )
    this.name = 'RegistrationRejectedError'
    this.code = init.code
    this.namespace = init.namespace
    this.worker_name = init.worker_name
    this.owner_worker_id = init.owner_worker_id
  }
}
