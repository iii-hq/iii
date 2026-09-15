package main

import (
	"context"
	"encoding/json"
	"log"
	"sync"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// This file implements a CUSTOM trigger TYPE — i.e. this worker becomes the thing the
// engine calls to start and stop trigger instances of type "interval". Mirrors
// trigger_type_example.rs. It's the counterpart to RegisterTrigger: there a worker
// asks the engine to fire a function when some built-in trigger matches; here the worker
// owns the trigger type and drives the invocations itself.

// intervalConfig is the per-instance config the engine hands us on registration.
type intervalConfig struct {
	EveryMs    int    `json:"every_ms"`
	FunctionID string `json:"-"` // filled from TriggerConfig.FunctionID
}

// intervalHandler implements iii.TriggerHandler for the "interval" trigger type: on
// register it starts a ticker that invokes the bound function every EveryMs; on
// unregister it stops the ticker. The client cancels ctx on shutdown, which also stops
// all tickers.
type intervalHandler struct {
	client *iii.Client

	mu      sync.Mutex
	tickers map[string]context.CancelFunc // trigger instance id -> stop
}

func newIntervalHandler(client *iii.Client) *intervalHandler {
	return &intervalHandler{client: client, tickers: map[string]context.CancelFunc{}}
}

func (h *intervalHandler) RegisterTrigger(ctx context.Context, cfg iii.TriggerConfig) error {
	var ic intervalConfig
	if err := json.Unmarshal(cfg.Config, &ic); err != nil {
		return err
	}
	if ic.EveryMs <= 0 {
		ic.EveryMs = 1000
	}

	// Tie the ticker's lifetime to ctx (cancelled on Close) and to its own stop func.
	tickerCtx, stop := context.WithCancel(ctx)
	h.mu.Lock()
	h.tickers[cfg.ID] = stop
	h.mu.Unlock()

	log.Printf("interval trigger %q registered: every %dms -> %s", cfg.ID, ic.EveryMs, cfg.FunctionID)
	go func() {
		t := time.NewTicker(time.Duration(ic.EveryMs) * time.Millisecond)
		defer t.Stop()
		for {
			select {
			case <-tickerCtx.Done():
				return
			case <-t.C:
				_, _ = h.client.Trigger(tickerCtx, iii.TriggerRequest{
					FunctionID: cfg.FunctionID,
					Data:       json.RawMessage(`{"source":"interval"}`),
					Action:     iii.VoidAction(),
				})
			}
		}
	}()
	return nil
}

func (h *intervalHandler) UnregisterTrigger(ctx context.Context, cfg iii.TriggerConfig) error {
	h.mu.Lock()
	stop := h.tickers[cfg.ID]
	delete(h.tickers, cfg.ID)
	h.mu.Unlock()
	if stop != nil {
		stop()
	}
	log.Printf("interval trigger %q unregistered", cfg.ID)
	return nil
}

// setupTriggerType registers the "interval" custom trigger type with the engine. Any
// worker (including this one) can then create "interval" trigger instances.
func setupTriggerType(client *iii.Client) {
	if err := client.RegisterTriggerType(
		"interval",
		"Invokes a function on a fixed millisecond interval",
		newIntervalHandler(client),
	); err != nil {
		log.Fatalf("register interval trigger type: %v", err)
	}
}
