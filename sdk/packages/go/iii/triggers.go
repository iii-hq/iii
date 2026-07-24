package iii

import (
	"context"
	"encoding/json"
)

// This file defines the worker-facing trigger-type API: how a worker that owns a
// custom trigger type (e.g. "cron", "webhook") is told to start and stop individual
// trigger instances. It mirrors TriggerConfig / TriggerHandler in
// sdk/packages/node/iii/src/triggers.ts.
//
// Two different "trigger" concepts live in this SDK; keep them distinct:
//
//   - Registering a trigger INSTANCE on the engine ("fire FunctionID when this http
//     route is hit") is done with Client.RegisterTrigger and the RegisterTriggerMessage
//     wire type in protocol.go.
//   - Implementing a custom trigger TYPE — being the worker the engine calls to set up
//     and tear down instances of that type — is what TriggerHandler below is for.

// TriggerConfig describes a single trigger instance handed to a TriggerHandler when
// the engine asks the worker to start or stop it. Mirrors TriggerConfig<TConfig> in
// triggers.ts.
//
// Config is left as raw JSON: the trigger type's owner knows its own config schema and
// unmarshals into a concrete type, the same way RegisterTriggerTypeMessage advertises
// that schema on the wire. This matches the rest of the protocol layer, which keeps
// schema-shaped payloads as json.RawMessage rather than forcing a Go generic through
// the whole client.
type TriggerConfig struct {
	// ID is the trigger instance ID.
	ID string `json:"id"`
	// FunctionID is the function to invoke when the trigger fires.
	FunctionID string `json:"function_id"`
	// Config is the trigger-type-specific configuration, as raw JSON.
	Config json.RawMessage `json:"config"`
	// Metadata is arbitrary metadata attached to the trigger instance.
	Metadata json.RawMessage `json:"metadata,omitempty"`
	// Namespace the target FunctionID resolves in. A provider that stores this
	// config and later fires the target must pass this namespace, or it fires in
	// the engine's default namespace. Empty means default.
	Namespace string `json:"namespace,omitempty"`
}

// TriggerHandler is implemented by a worker that owns a custom trigger type. When the
// engine registers a trigger instance of that type, it calls RegisterTrigger; when it
// removes one, it calls UnregisterTrigger. Mirrors the TriggerHandler interface in
// triggers.ts.
//
// The context is cancelled when the Client shuts down, so a handler that starts
// background work (a ticker, an HTTP listener) should tie that work's lifetime to it.
type TriggerHandler interface {
	// RegisterTrigger is called when a trigger instance is registered. The handler
	// should begin delivering invocations for cfg.FunctionID per cfg.Config.
	RegisterTrigger(ctx context.Context, cfg TriggerConfig) error
	// UnregisterTrigger is called when a trigger instance is removed. The handler
	// should stop and clean up whatever RegisterTrigger started for cfg.ID.
	UnregisterTrigger(ctx context.Context, cfg TriggerConfig) error
}
