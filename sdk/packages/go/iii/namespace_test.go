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

// TestTriggerSerializesNamespace verifies a TriggerRequest.Namespace reaches the
// wire on the invokefunction frame, so the caller can route to a namespace (and a
// provider can re-dispatch a namespaced TriggerConfig).
func TestTriggerSerializesNamespace(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)

	_, _ = c.Trigger(context.Background(), TriggerRequest{
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

// TestTriggerOmitsNamespaceWhenAbsent keeps the default path wire-clean: no
// namespace field when the request declares none.
func TestTriggerOmitsNamespaceWhenAbsent(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)

	_, _ = c.Trigger(context.Background(), TriggerRequest{
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
	if st := c.State(); st != StateFailed {
		t.Errorf("state = %q, want failed", st)
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
	if st := c.State(); st != StateConnected {
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

	if err := c.RegisterTriggerNamespaced("t1", "cron", "api::process", "  ", nil); err == nil {
		t.Fatal("a blank namespace on a binding must be refused")
	} else if !strings.Contains(err.Error(), "namespace is empty") {
		t.Fatalf("unexpected reason: %v", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_, err := c.Trigger(ctx, TriggerRequest{FunctionID: "api::ping", Namespace: " "})
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

	if err := c.RegisterTrigger("t1", "cron", "api::process", nil); err != nil {
		t.Fatalf("no namespace given is not a mistake: %v", err)
	}
}
