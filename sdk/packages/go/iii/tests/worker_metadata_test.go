//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Mirrors sdk/packages/node/iii/tests/register-worker-metadata.test.ts, but as a
// live-engine check: after connecting, this worker must appear in engine::workers::list
// tagged runtime "go".

// workerInfo mirrors the entries engine::workers::list returns (verified against the
// engine: runtime/status/os/version, no per-worker function array in this version).
type workerInfo struct {
	ID      string  `json:"id"`
	Name    *string `json:"name"`
	Runtime *string `json:"runtime"`
	Version *string `json:"version"`
	OS      *string `json:"os"`
	Status  string  `json:"status"`
}

// TestWorkerRegistersWithGoRuntime confirms the worker-metadata registration reaches the
// engine: after connecting, engine::workers::list contains a connected worker tagged
// runtime "go" (the engine's own builtins are runtime "engine"). The metadata register
// is fire-and-forget, so we give it a moment to land.
func TestWorkerRegistersWithGoRuntime(t *testing.T) {
	c := connect(t)
	if err := c.RegisterFunction("test::worker_meta::go::probe", func(_ context.Context, _ json.RawMessage) (any, error) {
		return nil, nil
	}); err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}
	settle()

	res, err := c.Trigger(ctxFor(t, 5*time.Second), iii.TriggerRequest{
		FunctionID: iii.FnListWorkers,
		Data:       json.RawMessage(`{}`),
	})
	if err != nil {
		t.Fatalf("engine::workers::list: %v", err)
	}
	var out struct {
		Workers []workerInfo `json:"workers"`
	}
	if err := json.Unmarshal(res, &out); err != nil {
		t.Fatalf("decode workers: %v\nraw: %s", err, res)
	}

	var ours *workerInfo
	for i := range out.Workers {
		w := &out.Workers[i]
		if w.Runtime != nil && *w.Runtime == "go" && w.Status == "connected" {
			ours = w
			break
		}
	}
	if ours == nil {
		t.Fatalf("no connected worker with runtime \"go\" in engine::workers::list (worker metadata not registered?)")
	}
	// Sanity-check the rest of the metadata we send.
	if ours.OS == nil || *ours.OS == "" {
		t.Error("worker os metadata is empty")
	}
	if ours.Version == nil || *ours.Version == "" {
		t.Error("worker version metadata is empty")
	}
}
