// Package iii is a Go SDK for the iii worker protocol.
//
// A process becomes an iii "worker" by opening a single WebSocket to the engine and
// registering functions and triggers; the engine invokes those functions and the worker
// replies over the same socket. The SDK handles the connection lifecycle (connect,
// reconnect with backoff, offline buffering) so callers work in terms of functions and
// triggers, not frames.
//
// # Quick start
//
// Create a worker, register a function, bind a trigger, and connect:
//
//	client := iii.RegisterWorker("") // III_URL, else ws://127.0.0.1:49134
//
//	client.RegisterFunction("hello::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
//		var req struct {
//			Body struct {
//				Name string `json:"name"`
//			} `json:"body"`
//		}
//		_ = json.Unmarshal(data, &req)
//		return map[string]any{
//			"status_code": 200,
//			"body":        map[string]string{"message": "Hello, " + req.Body.Name + "!"},
//		}, nil
//	})
//
//	client.RegisterTrigger("hello-http", "http", "hello::greet",
//		json.RawMessage(`{"api_path":"/greet","http_method":"POST"}`), nil)
//
//	if err := client.Connect(context.Background()); err != nil {
//		log.Fatal(err)
//	}
//	defer client.Close()
//
// An empty address means [EngineURLFromEnv]: the III_URL environment variable when it is
// set, else [DefaultEngineURL]. Options such as [WithName], [WithNamespace],
// [WithHeaders], [WithInvocationTimeout] and [WithTelemetry] configure the client.
//
// # Registering functions
//
// [Client.RegisterFunction] registers a handler; [RegisterFunctionOptions] adds a
// description, request and response JSON Schemas, metadata and observe hooks.
// [Client.RegisterHTTPFunction] registers a function hosted elsewhere that the engine
// invokes over HTTP ([HTTPInvocationRef], with [HMACAuth], [BearerAuth] or
// [APIKeyAuth]). Registrations are replayed on every reconnect until
// [Client.UnregisterFunction] (or [Client.UnregisterTrigger]) removes them.
//
// # Connection lifecycle
//
// [Client.OnConnectionStateChange] reports every [ConnectionState] change, in order.
// [Client.WaitUntilRegistered] blocks until the engine has acknowledged the worker on the
// current connection, or reports the terminal [Client.FatalError]. On each connection the
// worker announces itself with engine::workers::register, including its isolation
// (III_ISOLATION) and [TelemetryOptions].
//
// # Invoking functions
//
// [Client.Trigger] invokes a function and, by default, awaits its result. The
// [TriggerRequest] Action field selects the delivery semantics: the default (nil) awaits
// the result, [VoidAction] is fire-and-forget, and [EnqueueAction] routes through a named
// queue. A call waits [TriggerRequest].Timeout, or the client's [WithInvocationTimeout]
// (default [DefaultInvocationTimeout]). Errors are typed: [ErrTimeout] for a missed
// deadline, [ErrNotConnected] for a call cancelled by shutdown, and [InvocationError]
// (via errors.As) for a remote failure.
//
// # Observing functions
//
// Every function is observable: [Client.Observe] invokes a target function on each
// invocation of a source function that passes the observer's filters (the
// [filter] subpackage is the language). A function registered with [ObserveHooks]
// learns who observes it and can hand its own events to them with [Observer.Emit]
// and [Observer.Call]. Inside a handler, [InvocationFromContext] returns the
// current invocation; pass the handler's ctx to [Client.Trigger] so calls carry it
// as their caller. A function this worker registered runs in-process once the engine
// confirmed it for local dispatch.
//
// RegisterTrigger and RegisterTriggerType are deprecated in favor of Observe and
// ObserveHooks; they keep working and log a warning once per process.
//
// # Schema inference
//
// [RegisterFunctionTyped] infers JSON Schemas for a function's request and response from
// the Go types and advertises them to the engine — the Go counterpart of the Rust SDK's
// JsonSchema derive.
//
// # Channels
//
// For bulk or streaming data, [Client.CreateChannel] opens a streaming data channel; see
// [ChannelWriter] and [ChannelReader]. [Client.OpenReader] and [Client.OpenWriter] open
// the end of a channel whose [StreamChannelRef] arrived in a payload.
//
// The package mirrors the iii Node and Rust SDKs; see the repository README at
// https://github.com/iii-hq/iii for the engine and the other SDKs.
package iii
