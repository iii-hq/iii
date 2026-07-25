package iii

import (
	"context"
	"encoding/json"
	"errors"
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
	if st := c.State(); st != StateFailed {
		t.Errorf("state = %q, want failed", st)
	}
}
