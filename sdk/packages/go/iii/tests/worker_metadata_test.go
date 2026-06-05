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
// engine: after connecting, engine::workers::list contains THIS worker tagged runtime
// "go". The worker is created with a unique name (WithName) so the assertion correlates
// to the worker this test opened, rather than matching any connected go worker that may
// share the engine (which would let the test pass on the wrong worker — iii-hq/iii#1766).
// The metadata register is fire-and-forget, so we give it a moment to land.
func TestWorkerRegistersWithGoRuntime(t *testing.T) {
	workerName := "test-worker-meta-" + uniqueSuffix(t)
	c := connectNamed(t, workerName)
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

	// Find OUR worker by its unique name, not just any connected go worker.
	var ours *workerInfo
	for i := range out.Workers {
		w := &out.Workers[i]
		if w.Name != nil && *w.Name == workerName {
			ours = w
			break
		}
	}
	if ours == nil {
		t.Fatalf("this worker (%q) not found in engine::workers::list (metadata not registered?)", workerName)
	}
	if ours.Runtime == nil || *ours.Runtime != "go" {
		t.Errorf("runtime = %v, want \"go\"", ours.Runtime)
	}
	if ours.Status != "connected" {
		t.Errorf("status = %q, want connected", ours.Status)
	}
	if ours.OS == nil || *ours.OS == "" {
		t.Error("worker os metadata is empty")
	}
	if ours.Version == nil || *ours.Version == "" {
		t.Error("worker version metadata is empty")
	}
}
