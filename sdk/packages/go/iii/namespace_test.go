package iii

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"
)

// TestWorkerMetadataIncludesNamespace verifies WithNamespace rides on the
// engine::workers::register payload, so a Go worker registers into the declared
// namespace rather than always landing in default.
func TestWorkerMetadataIncludesNamespace(t *testing.T) {
	m := newMockEngine(t)
	connectClient(t, m, WithNamespace("orders"))

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstWhere(msgs, func(msg map[string]json.RawMessage) bool {
			return stringField(msg, "function_id") == FnRegisterWorker
		}) != nil
	}, 2*time.Second)

	reg := firstWhere(got, func(msg map[string]json.RawMessage) bool {
		return stringField(msg, "function_id") == FnRegisterWorker
	})
	if reg == nil {
		t.Fatal("no engine::workers::register frame sent on connect")
	}
	var meta workerMetadata
	if err := json.Unmarshal(reg["data"], &meta); err != nil {
		t.Fatalf("decode worker metadata: %v", err)
	}
	if meta.Namespace != "orders" {
		t.Errorf("namespace = %q, want orders", meta.Namespace)
	}
}

// TestNamespaceResolution checks the precedence: WithNamespace wins over the
// III_NAMESPACE env var, which wins over the default (empty).
func TestNamespaceResolution(t *testing.T) {
	t.Run("explicit option wins over env", func(t *testing.T) {
		t.Setenv("III_NAMESPACE", "from-env")
		c := New("ws://localhost:1", WithNamespace("from-option"))
		if c.namespace != "from-option" {
			t.Errorf("namespace = %q, want from-option", c.namespace)
		}
	})
	t.Run("env fallback when no option", func(t *testing.T) {
		t.Setenv("III_NAMESPACE", "from-env")
		c := New("ws://localhost:1")
		if c.namespace != "from-env" {
			t.Errorf("namespace = %q, want from-env", c.namespace)
		}
	})
	t.Run("default is empty", func(t *testing.T) {
		t.Setenv("III_NAMESPACE", "")
		c := New("ws://localhost:1")
		if c.namespace != "" {
			t.Errorf("namespace = %q, want empty default", c.namespace)
		}
	})
}

// TestCallSerializesNamespace verifies a CallRequest.Namespace reaches the
// wire on the invokefunction frame, so the caller can route to a namespace.
func TestCallSerializesNamespace(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)

	_, _ = c.Call(context.Background(), CallRequest{
		FunctionID: "state::get",
		Action:     VoidAction(),
		Namespace:  "analytics",
	})

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstWhere(msgs, func(msg map[string]json.RawMessage) bool {
			return stringField(msg, "function_id") == "state::get"
		}) != nil
	}, 2*time.Second)

	inv := firstWhere(got, func(msg map[string]json.RawMessage) bool {
		return stringField(msg, "function_id") == "state::get"
	})
	if inv == nil {
		t.Fatal("no invokefunction frame for state::get")
	}
	if ns := stringField(inv, "namespace"); ns != "analytics" {
		t.Errorf("namespace = %q, want analytics", ns)
	}
}

// TestCallOmitsNamespaceWhenAbsent keeps the default path wire-clean: no
// namespace field when the request declares none.
func TestCallOmitsNamespaceWhenAbsent(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)

	_, _ = c.Call(context.Background(), CallRequest{
		FunctionID: "state::get",
		Action:     VoidAction(),
	})

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstWhere(msgs, func(msg map[string]json.RawMessage) bool {
			return stringField(msg, "function_id") == "state::get"
		}) != nil
	}, 2*time.Second)
	inv := firstWhere(got, func(msg map[string]json.RawMessage) bool {
		return stringField(msg, "function_id") == "state::get"
	})
	if inv == nil {
		t.Fatal("no invokefunction frame for state::get")
	}
	if _, has := inv["namespace"]; has {
		t.Error("namespace field must be omitted when the request declares none")
	}
}

// TestEngineCallDefaultsToDefaultNamespace verifies that an engine-owned
// builtin cannot inherit or otherwise leak into a project namespace.
func TestEngineCallDefaultsToDefaultNamespace(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, WithNamespace("orders"))

	_, _ = c.Call(context.Background(), CallRequest{
		FunctionID: FnCreateChannel,
		Action:     VoidAction(),
	})

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstWhere(msgs, func(msg map[string]json.RawMessage) bool {
			return stringField(msg, "function_id") == FnCreateChannel
		}) != nil
	}, 2*time.Second)

	inv := firstWhere(got, func(msg map[string]json.RawMessage) bool {
		return stringField(msg, "function_id") == FnCreateChannel
	})
	if inv == nil {
		t.Fatal("no invokefunction frame for engine::channels::create")
	}
	if ns := stringField(inv, "namespace"); ns != "default" {
		t.Errorf("namespace = %q, want default", ns)
	}
}

// TestExplicitNamespaceWinsForEngineCall preserves strict explicit routing
// for callers that deliberately target a test or proxy namespace.
func TestExplicitNamespaceWinsForEngineCall(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m, WithNamespace("orders"))

	_, _ = c.Call(context.Background(), CallRequest{
		FunctionID: FnCreateChannel,
		Action:     VoidAction(),
		Namespace:  "sandbox",
	})

	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstWhere(msgs, func(msg map[string]json.RawMessage) bool {
			return stringField(msg, "function_id") == FnCreateChannel
		}) != nil
	}, 2*time.Second)

	inv := firstWhere(got, func(msg map[string]json.RawMessage) bool {
		return stringField(msg, "function_id") == FnCreateChannel
	})
	if inv == nil {
		t.Fatal("no invokefunction frame for engine::channels::create")
	}
	if ns := stringField(inv, "namespace"); ns != "sandbox" {
		t.Errorf("namespace = %q, want sandbox", ns)
	}
}

// TestImplicitCallInheritsTheWorkerNamespace: absent means this worker's
// namespace, as in the other SDKs. Engine-owned builtins stay in default (see
// TestEngineCallDefaultsToDefaultNamespace), and so does configuration::*,
// which belongs to the engine-owned configuration worker. Explicit always wins.
func TestImplicitCallInheritsTheWorkerNamespace(t *testing.T) {
	for _, tc := range []struct{ name, functionID, explicit, want string }{
		{"implicit call inherits", "api::ping", "", "orders"},
		{"implicit configuration call stays in default", "configuration::get", "", "default"},
		{"explicit wins over inheritance", "api::ping", "sandbox", "sandbox"},
		{"explicit wins for a configuration call", "configuration::get", "sandbox", "sandbox"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			m := newMockEngine(t)
			c := connectClient(t, m, WithNamespace("orders"))

			_, _ = c.Call(context.Background(), CallRequest{
				FunctionID: tc.functionID,
				Action:     VoidAction(),
				Namespace:  tc.explicit,
			})

			got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
				return indexOfFunctionID(msgs, tc.functionID) >= 0
			}, 2*time.Second)
			inv := got[indexOfFunctionID(got, tc.functionID)]
			if ns := stringField(inv, "namespace"); ns != tc.want {
				t.Errorf("namespace = %q, want %q", ns, tc.want)
			}
		})
	}
}

// TestObserverInheritsTheWorkerNamespace: an observer names two functions, and the
// functions a worker registers land in the worker's namespace, so an unset source or
// target namespace follows it. Naming another one stays possible.
func TestObserverInheritsTheWorkerNamespace(t *testing.T) {
	c := New("ws://unused.invalid", WithNamespace("orders"))
	t.Cleanup(func() { _ = c.Close() })
	for _, tc := range []struct {
		name     string
		source   Source
		opts     ObserverOptions
		wantFrom string
		wantTo   string
	}{
		{"unset inherits", Source{FunctionID: "api::process"}, ObserverOptions{}, "orders", "orders"},
		{"explicit wins", Source{FunctionID: "api::process", Namespace: "default"}, ObserverOptions{TargetNamespace: "default"}, "default", "default"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			h, err := c.Observe(tc.source, "api::audit", tc.opts)
			if err != nil {
				t.Fatal(err)
			}
			var msg ObserveMessage
			if err := json.Unmarshal(h.frame, &msg); err != nil {
				t.Fatal(err)
			}
			if msg.Source.Namespace != tc.wantFrom || msg.Target.Namespace != tc.wantTo {
				t.Errorf("source namespace %q, target namespace %q, want %q and %q", msg.Source.Namespace, msg.Target.Namespace, tc.wantFrom, tc.wantTo)
			}
		})
	}
}

// TestRegistrationRejectedIsFatal verifies a WORKER_NAMESPACE_CONFLICT is
// terminal: the client records the typed error, enters StateFailed, fails pending
// invocations, and does not reconnect (which would loop forever under the default
// MaxRetries of -1).
func TestRegistrationRejectedIsFatal(t *testing.T) {
	m := newMockEngine(t)
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if stringField(msg, "function_id") == FnRegisterWorker {
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = m.send(ctx, conn, &RegistrationRejectedMessage{
				Code:          "WORKER_NAMESPACE_CONFLICT",
				Namespace:     "orders",
				WorkerName:    "state",
				OwnerWorkerID: "owner-123",
			})
		}
	}

	c := New(m.url, WithName("state"), WithNamespace("orders"))
	c.startSupervisor()
	t.Cleanup(func() { _ = c.Close() })

	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) && c.FatalError() == nil {
		time.Sleep(20 * time.Millisecond)
	}

	err := c.FatalError()
	if err == nil {
		t.Fatal("FatalError not set after a WORKER_NAMESPACE_CONFLICT")
	}
	var re *RegistrationRejectedError
	if !errors.As(err, &re) {
		t.Fatalf("FatalError = %T, want *RegistrationRejectedError", err)
	}
	if re.Code != "WORKER_NAMESPACE_CONFLICT" || re.Namespace != "orders" {
		t.Errorf("got %+v, want code=WORKER_NAMESPACE_CONFLICT namespace=orders", re)
	}
	if re.WorkerName != "state" || re.FunctionID != "" {
		t.Errorf("got %+v, want only worker_name=state", re)
	}
	if st := c.ConnectionState(); st != StateFailed {
		t.Errorf("state = %q, want failed", st)
	}
}

// TestCallAfterFatalRejectionFailsAtOnce pins down that a call made after a fatal
// rejection (two clients in one process with the same name) fails with it at once,
// rather than waiting out its timeout for an engine that will never answer.
func TestCallAfterFatalRejectionFailsAtOnce(t *testing.T) {
	c := New("ws://127.0.0.1:0", WithName("state"), WithNamespace("orders"))
	c.handleRegistrationRejected(&RegistrationRejectedMessage{
		Code:          "WORKER_NAMESPACE_CONFLICT",
		Namespace:     "orders",
		WorkerName:    "state",
		OwnerWorkerID: "owner-123",
	})
	for _, action := range []*CallAction{nil, {Type: "void"}} {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		_, err := c.Call(ctx, CallRequest{FunctionID: "engine::functions::info", Action: action, Timeout: time.Minute})
		cancel()
		var re *RegistrationRejectedError
		if !errors.As(err, &re) || re.Code != "WORKER_NAMESPACE_CONFLICT" {
			t.Fatalf("action %v: Call = %v, want the RegistrationRejectedError", action, err)
		}
	}
	c.mu.Lock()
	pending := len(c.pending)
	c.mu.Unlock()
	if pending != 0 {
		t.Fatalf("%d pending calls, want none", pending)
	}
}

func TestFunctionRegistrationRejectedIsNonFatal(t *testing.T) {
	c := New("ws://127.0.0.1:0", WithName("state"), WithNamespace("orders"))
	c.mu.Lock()
	c.state = StateConnected
	c.mu.Unlock()

	c.handleRegistrationRejected(&RegistrationRejectedMessage{
		Code:          "FUNCTION_NAMESPACE_CONFLICT",
		Namespace:     "orders",
		FunctionID:    "state::get",
		OwnerWorkerID: "owner-123",
	})

	if err := c.FatalError(); err != nil {
		t.Fatalf("FatalError = %v, want nil", err)
	}
	if st := c.ConnectionState(); st != StateConnected {
		t.Errorf("state = %q, want connected", st)
	}
}

// TestBlankNamespaceIsRefused pins down that a namespace named and left blank
// is a mistake, not a way to ask for `default`.
//
// Go could not tell the two apart: WithNamespace("") left the field at its zero
// value, which is exactly what never calling it leaves, so the env fallback
// then ran and III_NAMESPACE could overwrite the choice the caller had made
// explicitly. Absent and blank ask for opposite things, and the failure is the
// one nobody sees: the worker serves from a namespace its declaration never
// named.
func TestBlankNamespaceIsRefused(t *testing.T) {
	for _, declared := range []string{"", "   "} {
		c := New("ws://127.0.0.1:1", WithNamespace(declared))

		err := c.FatalError()
		if err == nil {
			t.Fatalf("WithNamespace(%q) must be refused, got a healthy client", declared)
		}
		if !strings.Contains(err.Error(), "namespace is empty") {
			t.Fatalf("WithNamespace(%q): unexpected reason: %v", declared, err)
		}

		// Connect answers with that reason rather than dialling and blaming
		// retry exhaustion, which would describe the symptom.
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		connectErr := c.Connect(ctx)
		cancel()
		if !strings.Contains(fmt.Sprint(connectErr), "namespace is empty") {
			t.Fatalf("Connect should report the real cause, got: %v", connectErr)
		}
	}
}

// TestBlankNamespaceDoesNotFallBackToTheEnvironment is the half that made the
// old behaviour dangerous rather than merely wrong: an explicit blank fell
// through to the env fallback, so III_NAMESPACE silently won an argument the
// caller thought they had settled in the program text.
func TestBlankNamespaceDoesNotFallBackToTheEnvironment(t *testing.T) {
	t.Setenv("III_NAMESPACE", "from-the-environment")

	c := New("ws://127.0.0.1:1", WithNamespace(""))

	if c.FatalError() == nil {
		t.Fatal("an explicit blank must be refused, not replaced by the environment")
	}
	if c.namespace == "from-the-environment" {
		t.Fatal("III_NAMESPACE overwrote a namespace the caller declared")
	}
}

// TestAbsentNamespaceStillReadsTheEnvironment is the control: the env fallback
// is what carries managed workers, and refusing blank must not disturb it.
func TestAbsentNamespaceStillReadsTheEnvironment(t *testing.T) {
	t.Setenv("III_NAMESPACE", "orders")

	c := New("ws://127.0.0.1:1")

	if err := c.FatalError(); err != nil {
		t.Fatalf("no namespace given is not a mistake: %v", err)
	}
	if c.namespace != "orders" {
		t.Fatalf("expected the managed namespace, got %q", c.namespace)
	}
}

// TestBlankEnvironmentNamespaceIsLeftAlone: `FOO=` is how a shell says "not
// set" -- `III_NAMESPACE=${NS}` with NS unset produces exactly that -- so the
// env var is read as absent rather than refused. Only the option is a mistake.
func TestBlankEnvironmentNamespaceIsLeftAlone(t *testing.T) {
	t.Setenv("III_NAMESPACE", "  ")

	c := New("ws://127.0.0.1:1")

	if err := c.FatalError(); err != nil {
		t.Fatalf("a blank env var is absent, not a mistake: %v", err)
	}
	if c.namespace != "" {
		t.Fatalf("expected no namespace, got %q", c.namespace)
	}
}

// TestBlankCallNamespaceIsRefused: the same rule on the per-call path.
//
// Go dropped the empty string with `omitempty` and sent the call to the
// engine's default, while Node and Rust forwarded it verbatim and Python
// replaced it with the worker's. One mistake, four behaviours, none of them
// chosen -- each language's null-coalescing operator disagreed about "".
func TestBlankCallNamespaceIsRefused(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)

	if _, err := c.Observe(Source{FunctionID: "api::process", Namespace: "  "}, "api::audit"); err == nil {
		t.Fatal("a blank namespace on an observer must be refused")
	} else if !strings.Contains(err.Error(), "namespace is empty") {
		t.Fatalf("unexpected reason: %v", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_, err := c.Call(ctx, CallRequest{FunctionID: "api::ping", Namespace: " "})
	if err == nil {
		t.Fatal("a blank namespace on a call must be refused")
	}
	if !strings.Contains(err.Error(), "namespace is empty") {
		t.Fatalf("unexpected reason: %v", err)
	}
}

// TestAbsentCallNamespaceIsNotRefused is the control: absent is not blank, and
// a run that refuses everything must not be able to pass.
func TestAbsentCallNamespaceIsNotRefused(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)

	if _, err := c.Observe(Source{FunctionID: "api::process"}, "api::audit"); err != nil {
		t.Fatalf("no namespace given is not a mistake: %v", err)
	}
}
