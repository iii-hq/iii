package queue_test

import (
	"context"
	"encoding/json"
	"fmt"
	"log"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
	"github.com/iii-hq/iii/sdk/packages/go/iii/helpers/queue"
)

// ExampleEnqueueResult decodes the receipt the engine returns for an enqueued call.
func ExampleEnqueueResult() {
	raw := json.RawMessage(`{"messageReceiptId":"7d0c1f9e-receipt"}`)

	var receipt queue.EnqueueResult
	if err := json.Unmarshal(raw, &receipt); err != nil {
		panic(err)
	}
	fmt.Println(receipt.MessageReceiptID)
	// Output: 7d0c1f9e-receipt
}

// ExampleEnqueueResult_trigger enqueues a call on the "emails" queue and reads the
// receipt from the trigger result.
func ExampleEnqueueResult_trigger() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	res, err := client.Trigger(context.Background(), iii.TriggerRequest{
		FunctionID: "emails::send",
		Payload:    json.RawMessage(`{"to":"ada@example.com"}`),
		Action:     iii.EnqueueAction("emails"),
	})
	if err != nil {
		log.Fatal(err)
	}
	var receipt queue.EnqueueResult
	if err := json.Unmarshal(res, &receipt); err != nil {
		log.Fatal(err)
	}
	log.Printf("enqueued as %s", receipt.MessageReceiptID)
}
