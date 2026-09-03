package main

import (
	"context"
	"encoding/json"
	"log"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// setupCron registers a function and binds it to the engine's built-in "cron" trigger
// type, so the engine invokes it on a schedule. Mirrors the cron portion of
// cron_trigger_example.rs. The trigger config is raw JSON matching the engine's cron
// trigger schema (a standard cron expression).
func setupCron(client *iii.Client) {
	if err := client.RegisterFunction("example::scheduled_cleanup", func(ctx context.Context, data json.RawMessage) (any, error) {
		// The engine delivers a cron event payload; we just acknowledge the run.
		var ev struct {
			Trigger string `json:"trigger"`
			JobID   string `json:"job_id"`
		}
		_ = json.Unmarshal(data, &ev)
		log.Printf("cron fired: trigger=%s job_id=%s", ev.Trigger, ev.JobID)
		return map[string]any{"cleaned": true, "job_id": ev.JobID}, nil
	}); err != nil {
		log.Fatalf("register example::scheduled_cleanup: %v", err)
	}

	// "0 * * * * *" = at second 0 of every minute. The engine's cron trigger config field
	// is "expression" (6-field: sec min hour day month weekday) — see
	// engine/src/workers/cron/cron.rs and the Rust CronTriggerConfig.
	if err := client.RegisterTrigger(
		"example-cleanup-cron",
		"cron",
		"example::scheduled_cleanup",
		json.RawMessage(`{"expression":"0 * * * * *"}`),
		nil,
	); err != nil {
		log.Fatalf("register cron trigger: %v", err)
	}
}
