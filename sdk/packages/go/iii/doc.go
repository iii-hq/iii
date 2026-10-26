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
//	greet, err := client.RegisterFunction("hello::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
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
//	if err != nil {
//		log.Fatal(err)
//	}
//
//	if _, err := client.RegisterTrigger(iii.RegisterTriggerInput{
//		Type:       "http",
//		FunctionID: greet.ID,
//		Config:     json.RawMessage(`{"api_path":"/greet","http_method":"POST"}`),
//	}); err != nil {
//		log.Fatal(err)
//	}
//
//	if err := client.Connect(context.Background()); err != nil {
//		log.Fatal(err)
//	}
//	defer client.Shutdown(context.Background())
//
// An empty address means [EngineURLFromEnv]: the III_URL environment variable when it is
// set, else [DefaultEngineURL]. Options such as [WithName], [WithNamespace],
// [WithHeaders], [WithInvocationTimeout] and [WithTelemetry] configure the client.
//
// # Registering functions
//
// [Client.RegisterFunction] registers a handler and returns its [FunctionRef];
// [RegisterFunctionOptions] adds a description, request and response JSON Schemas,
// metadata and observe hooks. [Client.RegisterHTTPFunction] registers a function hosted
// elsewhere that the engine invokes over HTTP ([HTTPInvocationRef], with [HMACAuth],
// [BearerAuth] or [APIKeyAuth]). Registrations are replayed on every reconnect until the
// handle they returned removes them: [FunctionRef.Unregister], [Trigger.Unregister],
// [TriggerTypeRef.Unregister].
//
// # Connection lifecycle
//
// [Client.ConnectionState] is the current [ConnectionState], and
// [Client.OnConnectionStateChange] reports every change, in order.
// [Client.WaitUntilRegistered] blocks until the engine has acknowledged the worker on the
// current connection, or reports the terminal [Client.FatalError]; [Client.WorkerID] is
// the id the engine assigned. On each connection the worker announces itself with
// engine::workers::register, including its isolation (III_ISOLATION) and
// [TelemetryOptions].
//
// [Client.Shutdown] stops the worker gracefully within a ctx deadline: buffered reports
// and telemetry are flushed and the socket is closed with a closing handshake.
// [Client.Close] (io.Closer) stops it at once, with short fixed bounds on those flushes.
//
// # Invoking functions
//
// [Client.Trigger] invokes a function with [TriggerRequest].Payload and, by default,
// awaits its result. The [TriggerRequest] Action field selects the delivery semantics: the default (nil) awaits
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
// the Go types and advertises them to the engine. It is the Go counterpart of the Rust SDK's
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
