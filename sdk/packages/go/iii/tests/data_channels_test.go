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
// processor): make a pipe, stream data through the engine to another function, and
// assert the bytes arrive.

// TestChannelStreamSenderToProcessor: a sender writes JSON bytes into a pipe and hands
// the reader ref to a processor function, which reads the whole stream back and returns
// a count. Exercises Client.Pipe, OutputStream.Write/Close, a raw ref in a payload, and
// a received InputStream against the real engine.
func TestChannelStreamSenderToProcessor(t *testing.T) {
	c := connect(t)

	// Processor: the reader ref in its payload unmarshals to an InputStream.
	fn := "test::chan::go::processor-" + uniqueSuffix(t)
	if _, err := c.RegisterFunction(fn, func(ctx context.Context, data json.RawMessage) (any, error) {
		var in struct {
			Reader *iii.InputStream `json:"reader"`
		}
		if err := json.Unmarshal(data, &in); err != nil {
			return nil, err
		}
		if in.Reader == nil {
			return nil, &iii.InvocationError{Code: "missing_reader", Message: "no reader stream in payload"}
		}
		readCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
		defer cancel()
		raw, err := in.Reader.ReadAll(readCtx)
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

	// Sender side: make a pipe, stream the JSON, close, then call the processor with the
	// reader end embedded in the payload.
	reader, writer, err := c.Pipe(ctxFor(t, 5*time.Second))
	if err != nil {
		t.Fatalf("Pipe: %v", err)
	}
	payload := []byte(`[12,34,56,78,90]`) // count 5, sum 270
	writeCtx := ctxFor(t, 5*time.Second)
	if err := writer.Write(writeCtx, payload); err != nil {
		t.Fatalf("Write: %v", err)
	}
	if err := writer.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}

	procPayload, _ := json.Marshal(map[string]any{"label": "batch", "reader": reader})
	res, err := c.Call(ctxFor(t, 15*time.Second), iii.CallRequest{
		FunctionID: fn,
		Payload:    procPayload,
		Timeout:    15 * time.Second,
		Dispatch:   iii.DispatchRemote,
	})
	if err != nil {
		t.Fatalf("Call processor: %v", err)
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
