// Package queue holds the queue helper types of the iii Go SDK, the Go counterpart of
// the Node @iii-dev/helpers/queue module and the Rust iii_helpers::queue module.
//
// A function invoked with iii.EnqueueAction is not run synchronously: the engine puts
// the call on the named queue and answers the trigger with an [EnqueueResult] receipt
// instead of the function's result.
package queue

// EnqueueResult is the result returned when a function is invoked with
// iii.EnqueueAction (TriggerAction.Enqueue in the Node SDK). Decode the
// json.RawMessage returned by iii.Client.Trigger into it.
type EnqueueResult struct {
	// MessageReceiptID is the unique receipt ID of the enqueued message.
	MessageReceiptID string `json:"messageReceiptId"`
}
