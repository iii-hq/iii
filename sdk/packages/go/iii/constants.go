package iii

import (
	"os"
	"strings"
	"time"
)

// DefaultEngineURL is the worker WebSocket endpoint of a locally-running engine, used
// when neither an explicit address nor III_URL is set (see [EngineURLFromEnv]). The
// engine listens for workers here by default; its HTTP API is on :3111. The IPv4
// loopback is spelled out on purpose: "localhost" can resolve to ::1 on a host whose
// engine only listens on IPv4. Same value as DEFAULT_ENGINE_URL in the Node, Python
// and Rust SDKs.
const DefaultEngineURL = "ws://127.0.0.1:49134"

// EngineURLFromEnv returns the engine address a worker connects to when its code names
// none: the III_URL environment variable, trimmed, when it is not blank, else
// [DefaultEngineURL]. The supervisor that spawned the process (iii compose, a container
// runtime, systemd) sets III_URL the same way it sets III_NAMESPACE and
// III_WORKER_NAME. [New] and [RegisterWorker] call it when their url argument is empty,
// so an explicit address wins, then III_URL, then the default, as in the Node SDK.
func EngineURLFromEnv() string {
	if url := strings.TrimSpace(os.Getenv("III_URL")); url != "" {
		return url
	}
	return DefaultEngineURL
}

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
	// TriggerFunctionsAvailable is the trigger type fired when the function set
	// changes.
	//
	// Deprecated: observe the engine-hosted function [FnOnFunctionsChanged] with
	// [Client.Observe] instead. Migration guide:
	// https://iii.dev/docs/upgrading/from-0-24-x
	TriggerFunctionsAvailable = "engine::functions-available"
	// TriggerLog is the trigger type fired for each engine log record.
	//
	// Deprecated: observe [FnOnLog] with [Client.Observe] (filter on input.level)
	// instead. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x
	TriggerLog = "log"
)

// Engine-hosted source functions (auto delivery): observe them with [Client.Observe]
// and filters, in place of the engine trigger types.
const (
	FnOnFunctionsChanged    = "engine::on-functions-changed"
	FnOnWorkersChanged      = "engine::on-workers-changed"
	FnOnLog                 = "observability::on-log"
	FnOnSpan                = "observability::on-span"
	FnOnConfigurationUpdate = "configuration::on-updated"
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
// before failing with ErrTimeout, unless [WithInvocationTimeout] or
// [TriggerRequest].Timeout says otherwise. Mirrors DEFAULT_INVOCATION_TIMEOUT_MS (30000)
// in iii-constants.ts.
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
