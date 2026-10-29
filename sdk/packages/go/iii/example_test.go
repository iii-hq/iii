package iii_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// Example_helloWorker is a complete hello-world worker: register a function, route HTTP
// requests to it by observing the http worker's http::on-request, and connect. The
// handler receives the request (the body is under "body") and returns the response
// ({status_code, body}).
func Example_helloWorker() {
	client := iii.RegisterWorker(iii.DefaultEngineURL) // ws://127.0.0.1:49134

	greet, err := client.RegisterFunction("hello::greet", func(ctx context.Context, data json.RawMessage) (any, error) {
		var req struct {
			Body struct {
				Name string `json:"name"`
			} `json:"body"`
		}
		if err := json.Unmarshal(data, &req); err != nil {
			return nil, err
		}
		return map[string]any{
			"status_code": 200,
			"body":        map[string]string{"message": "Hello, " + req.Body.Name + "!"},
		}, nil
	})
	if err != nil {
		log.Fatal(err)
	}

	if _, err := client.Observe(iii.Source{
		FunctionID: "http::on-request",
		Args:       iii.Args{"method": "POST", "path": "/greet"},
	}, greet.ID); err != nil {
		log.Fatal(err)
	}

	if err := client.Connect(context.Background()); err != nil {
		log.Fatal(err)
	}
	defer client.Close()
}

// ExampleFunctionRef_Unregister removes a function through the handle its registration
// returned; a reconnect no longer registers it.
func ExampleFunctionRef_Unregister() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	ref, err := client.RegisterFunction("jobs::nightly", func(ctx context.Context, data json.RawMessage) (any, error) {
		return nil, nil
	})
	if err != nil {
		log.Fatal(err)
	}
	// Later, when the job is retired:
	if err := ref.Unregister(); err != nil {
		log.Fatal(err)
	}
}

// ExampleClient_Shutdown stops a worker gracefully on SIGTERM-style exits: buffered
// reports and telemetry get up to five seconds to reach the engine.
func ExampleClient_Shutdown() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := client.Shutdown(ctx); err != nil {
		log.Printf("shutdown cut short: %v", err)
	}
}

// ExampleClient_RegisterFunction_withMetadata attaches registration metadata via the
// optional options value; the handler signature is unchanged.
func ExampleClient_RegisterFunction_withMetadata() {
	client := iii.New(iii.DefaultEngineURL)

	handler := func(ctx context.Context, data json.RawMessage) (any, error) {
		return map[string]any{"ok": true}, nil
	}

	client.RegisterFunction("orders::create", handler, iii.RegisterFunctionOptions{
		Metadata: json.RawMessage(`{"owner":"billing-team","priority":"high"}`),
	})
}

// ExampleMetadataFromContext reads the optional per-invocation metadata sidecar from the
// handler's ctx; ok is false when the caller attached none.
func ExampleMetadataFromContext() {
	client := iii.New(iii.DefaultEngineURL)

	client.RegisterFunction("orders::create",
		func(ctx context.Context, data json.RawMessage) (any, error) {
			metadata, ok := iii.MetadataFromContext(ctx)
			return map[string]any{"ok": true, "has_metadata": ok, "metadata": metadata}, nil
		})
}

// ExampleClient_Call invokes a function and awaits its result, then shows the typed
// error handling: ErrTimeout for a missed deadline and InvocationError for a remote
// failure.
func ExampleClient_Call() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	res, err := client.Call(context.Background(), iii.CallRequest{
		FunctionID: "orders::create",
		Payload:    json.RawMessage(`{"item":"widget"}`),
	})
	switch {
	case errors.Is(err, iii.ErrTimeout):
		fmt.Println("timed out")
	case err != nil:
		var ie *iii.InvocationError
		if errors.As(err, &ie) {
			fmt.Println("remote error:", ie.Code)
		}
	default:
		fmt.Printf("result: %s\n", res)
	}
}

// greetRequest and greetResponse describe a function's contract; their JSON Schemas are
// inferred and advertised to the engine by RegisterFunctionTyped.
type greetRequest struct {
	Name string `json:"name" jsonschema:"required"`
}

type greetResponse struct {
	Message string `json:"message"`
}

// ExampleRegisterFunctionTyped registers a function whose request and response schemas
// are inferred from the Go types, the Go counterpart of the Rust SDK's JsonSchema derive.
func ExampleRegisterFunctionTyped() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	iii.RegisterFunctionTyped(client, "hello::greet",
		func(ctx context.Context, req greetRequest) (greetResponse, error) {
			return greetResponse{Message: "Hello, " + req.Name + "!"}, nil
		})
}

// ExampleClient_ObserveFunction runs a target on every successful call of a function.
func ExampleClient_ObserveFunction() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	client.RegisterFunction("alerts::new-order", func(ctx context.Context, data json.RawMessage) (any, error) {
		var event iii.Observed
		if err := json.Unmarshal(data, &event); err != nil {
			return nil, err
		}
		log.Printf("order created: %s", event.Output)
		return nil, nil
	})
	if _, err := client.ObserveFunction("orders::create", "alerts::new-order"); err != nil {
		log.Fatal(err)
	}
}

// ExampleMatchKeys hosts an event source: one call of timer::on-tick per tick of each
// interval its observers ask for, matched by keys to the observers of that interval
// only. The hooks are set by id before the registration, so they run for every
// observer.
func ExampleMatchKeys() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	type timerArgs struct {
		EveryMs int64 `json:"every_ms"`
	}
	keyOf := func(everyMs int64) string { return fmt.Sprint(everyMs) }
	stops := map[string]chan struct{}{} // one ticker per interval, used on the hook queue only

	client.OnObserverAddedFor("timer::on-tick", func(ctx context.Context, obs *iii.Observer) error {
		var args timerArgs
		if err := obs.DecodeArgs(&args); err != nil || args.EveryMs <= 0 {
			return iii.ObserveRejection("INVALID_ARGS", "every_ms must be a positive integer")
		}
		if _, running := stops[keyOf(args.EveryMs)]; running {
			return nil
		}
		stop := make(chan struct{})
		stops[keyOf(args.EveryMs)] = stop
		go func() {
			ticker := time.NewTicker(time.Duration(args.EveryMs) * time.Millisecond)
			defer ticker.Stop()
			for {
				select {
				case <-stop:
					return
				case now := <-ticker.C:
					tick, _ := json.Marshal(map[string]int64{"every_ms": args.EveryMs, "fired_at_ms": now.UnixMilli()})
					if !client.IsObservedFor("timer::on-tick", &iii.Call{Input: tick}) {
						continue
					}
					client.Call(context.Background(), iii.CallRequest{FunctionID: "timer::on-tick", Payload: tick, Action: iii.VoidAction()})
				}
			}
		}()
		return nil
	})
	client.OnObserverRemovedFor("timer::on-tick", func(ctx context.Context, obs *iii.Observer) {
		var args timerArgs
		_ = obs.DecodeArgs(&args)
		for _, other := range client.Observers("timer::on-tick") {
			var o timerArgs
			if other != obs && other.DecodeArgs(&o) == nil && o.EveryMs == args.EveryMs {
				return // the interval is still observed
			}
		}
		if stop, ok := stops[keyOf(args.EveryMs)]; ok {
			close(stop)
			delete(stops, keyOf(args.EveryMs))
		}
	})
	client.OnObserverMatchFor("timer::on-tick", iii.MatchKeys(
		func(obs *iii.Observer) []string { // what the observer listens for
			var args timerArgs
			_ = obs.DecodeArgs(&args)
			return []string{keyOf(args.EveryMs)}
		},
		func(call *iii.Call) []string { // what the call is: the tick carries every_ms too
			var tick timerArgs
			_ = json.Unmarshal(call.Input, &tick)
			return []string{keyOf(tick.EveryMs)}
		},
	))

	if _, err := client.RegisterFunction("timer::on-tick", iii.Identity, iii.FunctionOptions{
		Description: "One call per tick. Observe it with args { every_ms }.",
		ArgsFormat:  json.RawMessage(`{"type":"object","required":["every_ms"],"properties":{"every_ms":{"type":"integer","minimum":1}}}`),
		Private:     true,
	}); err != nil {
		log.Fatal(err)
	}
}

// ExampleClient_OnObserverDeliverFor routes each chat command to the one observer that
// handles it: the match hook picks the observer of the command, and the deliver hook
// calls its target through it and answers the caller with the target's result.
func ExampleClient_OnObserverDeliverFor() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	commandOf := func(call *iii.Call) string {
		var req struct {
			Command string `json:"command"`
		}
		_ = json.Unmarshal(call.Input, &req)
		return req.Command
	}
	client.OnObserverMatchFor("chat::on-command", iii.MatchFunc(func(obs *iii.Observer, call *iii.Call) bool {
		return obs.Args["command"] == commandOf(call)
	}))
	client.OnObserverDeliverFor("chat::on-command", func(ctx context.Context, call *iii.Call, observers []*iii.Observer) (any, error) {
		if len(observers) == 0 {
			return map[string]string{"reply": "unknown command /" + commandOf(call)}, nil
		}
		obs := observers[0]
		if !obs.Matches(call) {
			return map[string]string{"reply": "/" + commandOf(call) + " is not available"}, nil
		}
		// what this hook returns is what the caller gets back
		return client.Call(ctx, iii.CallRequest{Observer: obs, Payload: call.Input})
	})

	if _, err := client.RegisterFunction("chat::on-command", iii.Identity, iii.FunctionOptions{
		ArgsFormat: json.RawMessage(`{"type":"object","required":["command"],"properties":{"command":{"type":"string"}}}`),
		Private:    true,
	}); err != nil {
		log.Fatal(err)
	}
}
