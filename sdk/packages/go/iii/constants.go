package iii

import "time"

// DefaultEngineURL is the worker WebSocket endpoint of a locally-running engine.
// The engine listens for workers here by default; its HTTP API is on :3111.
// Mirrors the address used throughout the Node and Rust SDK docs
// (sdk/packages/node/iii/src/iii.ts, sdk/packages/rust/iii/src/iii.rs).
const DefaultEngineURL = "ws://localhost:49134"

// runtimeTag is this SDK's value for the worker-metadata "runtime" field, sent on
// engine::workers::register. The engine tags each worker by the SDK that registered
// it: the Node SDK sends "node" (iii.ts:529) and the Rust SDK "rust" (iii.rs:279);
// the Go SDK adds "go".
const runtimeTag = "go"

// EngineFunctions are the engine's built-in function paths a worker can invoke via
// Trigger. Mirrors EngineFunctions in sdk/packages/node/iii/src/iii-constants.ts.
//
// Naming note (kept verbatim from the Node SDK so the two read the same): ListTriggers
// / InfoTriggers cover trigger TYPES (templates); ListRegisteredTriggers /
// InfoRegisteredTriggers cover trigger INSTANCES (subscriber rows).
const (
	FnListFunctions          = "engine::functions::list"
	FnInfoFunctions          = "engine::functions::info"
	FnListWorkers            = "engine::workers::list"
	FnInfoWorkers            = "engine::workers::info"
	FnListTriggers           = "engine::triggers::list"
	FnInfoTriggers           = "engine::triggers::info"
	FnListRegisteredTriggers = "engine::registered-triggers::list"
	FnInfoRegisteredTriggers = "engine::registered-triggers::info"
	FnRegisterWorker         = "engine::workers::register"
	FnCreateChannel          = "engine::channels::create"
)

// Engine trigger types. Mirrors EngineTriggers in iii-constants.ts.
const (
	TriggerFunctionsAvailable = "engine::functions-available"
	TriggerLog                = "log"
)

// Log function paths. Mirrors LogFunctions in iii-constants.ts. Invoke these with
// Trigger (fire-and-forget) to write to the engine log.
const (
	LogInfo  = "engine::log::info"
	LogWarn  = "engine::log::warn"
	LogError = "engine::log::error"
	LogDebug = "engine::log::debug"
)

// DefaultInvocationTimeout bounds how long Trigger waits for an InvocationResult
// before failing with ErrTimeout. Mirrors DEFAULT_INVOCATION_TIMEOUT_MS (30000) in
// iii-constants.ts.
const DefaultInvocationTimeout = 30 * time.Second

// ReconnectConfig controls the exponential-backoff-with-jitter reconnect loop. The
// defaults below match the Node SDK's DEFAULT_BRIDGE_RECONNECTION_CONFIG
// (iii-constants.ts) exactly, so a Go worker reconnects on the same schedule as a
// Node one.
type ReconnectConfig struct {
	// InitialDelay is the delay before the first reconnect attempt.
	InitialDelay time.Duration
	// MaxDelay caps the backoff.
	MaxDelay time.Duration
	// BackoffMultiplier scales the delay after each failed attempt.
	BackoffMultiplier float64
	// JitterFactor (0..1) randomizes each delay by up to ±JitterFactor to avoid a
	// thundering herd of workers reconnecting in lockstep.
	JitterFactor float64
	// MaxRetries caps reconnect attempts; -1 means retry forever (the default).
	MaxRetries int
}

// DefaultReconnectConfig is the reconnect schedule shared with the Node SDK:
// start at 1s, double each time, cap at 30s, ±30% jitter, retry forever.
func DefaultReconnectConfig() ReconnectConfig {
	return ReconnectConfig{
		InitialDelay:      1 * time.Second,
		MaxDelay:          30 * time.Second,
		BackoffMultiplier: 2,
		JitterFactor:      0.3,
		MaxRetries:        -1,
	}
}

// ConnectionState is the lifecycle state of a Client's WebSocket. Mirrors
// IIIConnectionState in iii-constants.ts.
type ConnectionState string

const (
	StateDisconnected ConnectionState = "disconnected"
	StateConnecting   ConnectionState = "connecting"
	StateConnected    ConnectionState = "connected"
	StateReconnecting ConnectionState = "reconnecting"
	StateFailed       ConnectionState = "failed"
)
