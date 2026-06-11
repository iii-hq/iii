/**
 * Result returned when a function is invoked with `TriggerAction.Enqueue`.
 */
export type EnqueueResult = {
  /** Unique receipt ID for the enqueued message. */
  messageReceiptId: string
}
