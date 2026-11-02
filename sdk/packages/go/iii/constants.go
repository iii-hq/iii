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

// EngineFunctions are the engine's built-in function paths a worker can invoke with
// [Client.Call]. Mirrors EngineFunctions in the Node SDK. A function row of
// engine::functions::list and engine::functions::info carries observer_count, and
// args_format, private and observable when set.
const (
	FnListFunctions  = "engine::functions::list"
	FnInfoFunctions  = "engine::functions::info"
	FnListWorkers    = "engine::workers::list"
	FnInfoWorkers    = "engine::workers::info"
	FnRegisterWorker = "engine::workers::register"
	FnCreateChannel  = "engine::channels::create"
)

// Engine-hosted sources: private functions of the engine whose handler returns the
// event, so input and output are both the event. Observe them with [Client.Observe]
// and filters on input; only the engine calls them. [EngineSources] names them as the
// other SDKs do.
const (
	// FnOnFunctionsChanged is called when the function set changes:
	// { event: "functions_changed", functions: [...] }.
	FnOnFunctionsChanged = "engine::on-functions-changed"
	// FnOnWorkersChanged is called when a worker connects, disconnects or updates its
	// metadata: { event, worker_id }.
	FnOnWorkersChanged = "engine::on-workers-changed"
	// FnOnObserversChanged is called once per observer state change:
	// { observer_id, function_id, target, state, reason? }.
	FnOnObserversChanged = "engine::on-observers-changed"
	// FnOnLog is called once per log record.
	FnOnLog = "observability::on-log"
	// FnOnSpan is called once per window of recorded spans: { trace_ids }.
	FnOnSpan = "observability::on-span"
	// FnOnConfigurationUpdate is called when a configuration entry is registered,
	// updated or deleted.
	FnOnConfigurationUpdate = "configuration::on-updated"
	// FnOnStreamChanged is called on every change of a stream item (iii-stream).
	FnOnStreamChanged = "stream::on-changed"
	// FnOnStreamJoin is called when a client joins a stream group (iii-stream).
	FnOnStreamJoin = "stream::on-join"
	// FnOnStreamLeave is called when a client leaves a stream group (iii-stream).
	FnOnStreamLeave = "stream::on-leave"
)

// EngineSources are the ids of the engine-hosted sources under the names every SDK
// gives them (EngineSources in the Node, browser, Python and Rust SDKs):
// EngineSources.FunctionsChanged is [FnOnFunctionsChanged], and so on.
//
//	client.ObserveFunction(iii.EngineSources.FunctionsChanged, "catalog::refresh")
var EngineSources = struct {
	FunctionsChanged     string
	WorkersChanged       string
	ObserversChanged     string
	Log                  string
	Span                 string
	ConfigurationUpdated string
	StreamChanged        string
	StreamJoin           string
	StreamLeave          string
}{
	FunctionsChanged:     FnOnFunctionsChanged,
	WorkersChanged:       FnOnWorkersChanged,
	ObserversChanged:     FnOnObserversChanged,
	Log:                  FnOnLog,
	Span:                 FnOnSpan,
	ConfigurationUpdated: FnOnConfigurationUpdate,
	StreamChanged:        FnOnStreamChanged,
	StreamJoin:           FnOnStreamJoin,
	StreamLeave:          FnOnStreamLeave,
}

// Log function paths. Mirrors LogFunctions in the Node SDK. Call these with
// [VoidAction] (fire-and-forget) to write to the engine log.
const (
	LogInfo  = "engine::log::info"
	LogWarn  = "engine::log::warn"
	LogError = "engine::log::error"
	LogDebug = "engine::log::debug"
)

// DefaultInvocationTimeout bounds how long Call waits for an InvocationResult before
// failing with ErrTimeout, unless [WithInvocationTimeout] or [CallRequest].Timeout says
// otherwise. Same default (30000 ms) as
// DEFAULT_INVOCATION_TIMEOUT_MS in the Node SDK.
const DefaultInvocationTimeout = 30 * time.Second

// ReconnectConfig controls the exponential-backoff-with-jitter reconnect loop. The
// defaults below match the Node SDK's DEFAULT_BRIDGE_RECONNECTION_CONFIG exactly, so
// a Go worker reconnects on the same schedule as a Node one.
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
// IIIConnectionState in the Node SDK.
type ConnectionState string

const (
	StateDisconnected ConnectionState = "disconnected"
	StateConnecting   ConnectionState = "connecting"
	StateConnected    ConnectionState = "connected"
	StateReconnecting ConnectionState = "reconnecting"
	StateFailed       ConnectionState = "failed"
)
