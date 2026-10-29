/**
 * Result returned when a function is called with `CallAction.Enqueue`.
 */
export type EnqueueResult = {
  /** Unique receipt ID for the enqueued message. */
  messageReceiptId: string
}
