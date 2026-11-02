// Package iii is a Go SDK for the iii worker protocol.
//
// A process becomes an iii "worker" by opening a single WebSocket to the engine and
// registering functions; the engine invokes those functions and the worker replies over
// the same socket. Functions are observable: when a function runs, its observers run.
// The SDK handles the connection lifecycle (connect, reconnect with backoff, offline
// buffering), so callers work in terms of functions and observers.
//
// # Quick start
//
// Create a worker, register a function, route HTTP requests to it, and connect:
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
//	// POST /greet runs hello::greet: it observes the http worker's http::on-request
//	if _, err := client.Observe(iii.Source{
//		FunctionID: "http::on-request",
//		Args:       iii.Args{"method": "POST", "path": "/greet"},
//	}, greet.ID); err != nil {
//		log.Fatal(err)
//	}
//
//	if err := client.Connect(context.Background()); err != nil {
//		log.Fatal(err)
//	}
//	defer func() {
//		// Graceful stop: buffered reports and telemetry get up to five seconds.
//		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
//		defer cancel()
//		if err := client.Shutdown(ctx); err != nil {
//			log.Printf("shutdown cut short: %v", err)
//		}
//	}()
//
// An empty address means [EngineURLFromEnv]: the III_URL environment variable when it is
// set, else [DefaultEngineURL]. Options such as [WithName], [WithNamespace],
// [WithHeaders], [WithInvocationTimeout] and [WithTelemetry] configure the client.
//
// # Registering functions
//
// [Client.RegisterFunction] registers a handler and returns its [FunctionRef] (an id is
// registered once per client, as in the other SDKs); [RegisterFunctionOptions] adds a
// description, request and response JSON Schemas, metadata, and what describes the
// function to observers: ArgsFormat, Private, Observable and Standby.
// [Client.RegisterHTTPFunction] registers a function hosted elsewhere that the engine
// invokes over HTTP ([HTTPInvocationRef], with [HMACAuth], [BearerAuth] or
// [APIKeyAuth]). Registrations are replayed on every reconnect until
// [FunctionRef.Unregister] removes them.
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
// # Calling functions
//
// [Client.Call] invokes a function with [CallRequest].Payload and, by default, awaits
// its result. The [CallRequest] Action field selects the delivery semantics: the
// default (nil) awaits the result, [VoidAction] is fire-and-forget, and [EnqueueAction]
// routes through a named queue. A call waits [CallRequest].Timeout, or the client's
// [WithInvocationTimeout] (default [DefaultInvocationTimeout]). Errors are typed:
// [ErrTimeout] for a missed deadline, [ErrNotConnected] for a call cancelled by
// shutdown, and [InvocationError] (via errors.As) for a remote failure.
// [Client.Trigger], [TriggerRequest] and [TriggerAction] are the same function and
// types under their 0.25 names.
//
// # Observing functions
//
// Every function is observable: [Client.Observe] (or [Client.ObserveFunction]) runs a
// target function on each call of a source function that passes the observer's filters
// (the [filter] subpackage is the language). Inside a handler, [InvocationFromContext]
// returns the current invocation; pass the handler's ctx to [Client.Call] so calls carry
// it as their caller. A function this worker registered runs in-process once the
// engine confirmed it for local dispatch.
//
// Filters run where the call is. The engine evaluates the observers' on and filters for
// every call it routes: it holds the call's input, its output or error, its caller and
// its metadata. The worker that hosts a function evaluates them for the calls it makes
// to that function in-process, which the engine never sees, and in [Observer.Matches].
// Both match through an index of the observers' equality conditions: each observer is
// filed under one $eq of a scalar, or one $in of scalars, on a path of its filter's
// top-level $and, and a call evaluates the whole filter of only the observers filed
// under its value there and of those without such a condition. With 1,000 observers that each filter on their
// own input.tenant, a call evaluates one filter.
//
// # Hosting observed functions
//
// The worker that hosts a function decides what observing it means with four hooks,
// each overriding a default: [AddedHook] (accept the observer; set something up from
// its Args, or reject it with [ObserveRejection]), [RemovedHook] (forget it; tear
// down), [MatchHook] (every observer wants every call; say which observers want each
// call) and [DeliverHook] (iii delivers each call to the matched observers whose on and
// filters match it, and the caller gets the handler's result; deliver the call yourself
// to the matched observers and decide what the caller gets back).
// [Client.OnObserverAdded] and [Client.OnObserverRemoved] set the first two for every
// function the worker hosts; [Client.OnObserverAddedFor], [Client.OnObserverRemovedFor],
// [Client.OnObserverMatchFor] and [Client.OnObserverDeliverFor] set them for one
// function by id, and the [FunctionRef] methods for the function they were returned
// for. Set hooks by id before RegisterFunction. [MatchFunc] matches with a predicate,
// [MatchKeys] with keys the SDK indexes, and [FunctionRef.IsObservedFor] answers
// whether a call would reach anyone. A deliver hook calls a target itself with
// [CallRequest].Observer, after [Observer.Matches].
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
