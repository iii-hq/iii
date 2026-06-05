//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Mirrors sdk/packages/rust/iii/tests/data_channels.rs (stream_data_from_sender_to_
// processor and bidirectional_streaming): create a channel, stream data through the
// engine to another function, and assert the bytes + interleaved messages arrive.

// TestChannelStreamSenderToProcessor: a sender writes JSON bytes into a channel and
// hands the reader ref to a processor function, which reads the whole stream back and
// returns a count. Exercises CreateChannel, Writer.Write/Close, ref-in-payload travel,
// ExtractChannelRefs, OpenReader, Reader.ReadAll against the real engine.
func TestChannelStreamSenderToProcessor(t *testing.T) {
	c := connect(t)

	// Processor: pull the reader ref out of its payload, read the full stream, count items.
	if err := c.RegisterFunction("test::chan::go::processor", func(ctx context.Context, data json.RawMessage) (any, error) {
		refs, err := iii.ExtractChannelRefs(data)
		if err != nil {
			return nil, err
		}
		ref, ok := refs["reader"]
		if !ok || ref.Direction != iii.ChannelRead {
			return nil, &iii.InvocationError{Code: "missing_reader", Message: "no reader channel ref in payload"}
		}
		reader := iii.OpenReader(c.Address(), ref)
		readCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
		defer cancel()
		raw, err := reader.ReadAll(readCtx)
		if err != nil {
			return nil, err
		}
		var nums []int
		if err := json.Unmarshal(raw, &nums); err != nil {
			return nil, err
		}
		sum := 0
		for _, n := range nums {
			sum += n
		}
		return map[string]any{"count": len(nums), "sum": sum}, nil
	}); err != nil {
		t.Fatalf("RegisterFunction processor: %v", err)
	}
	settle()

	// Sender side: create a channel, stream the JSON, close, then trigger the processor
	// with the reader ref embedded in the payload.
	ch, err := c.CreateChannel(ctxFor(t, 5*time.Second), nil)
	if err != nil {
		t.Fatalf("CreateChannel: %v", err)
	}

	payload := []byte(`[12,34,56,78,90]`) // count 5, sum 270
	writeCtx := ctxFor(t, 5*time.Second)
	if err := ch.Writer.Write(writeCtx, payload); err != nil {
		t.Fatalf("Writer.Write: %v", err)
	}
	if err := ch.Writer.Close(); err != nil {
		t.Fatalf("Writer.Close: %v", err)
	}

	// Embed the reader ref in the processor's payload, the way the Rust test does.
	procPayload, _ := json.Marshal(map[string]any{
		"label":  "batch",
		"reader": ch.ReaderRef,
	})
	res, err := c.Trigger(ctxFor(t, 15*time.Second), iii.TriggerRequest{
		FunctionID: "test::chan::go::processor",
		Data:       procPayload,
		Timeout:    15 * time.Second,
	})
	if err != nil {
		t.Fatalf("Trigger processor: %v", err)
	}
	var out struct {
		Count int `json:"count"`
		Sum   int `json:"sum"`
	}
	if err := json.Unmarshal(res, &out); err != nil {
		t.Fatalf("decode processor result: %v\nraw: %s", err, res)
	}
	if out.Count != 5 || out.Sum != 270 {
		t.Errorf("processor result = %+v, want count=5 sum=270", out)
	}
}
