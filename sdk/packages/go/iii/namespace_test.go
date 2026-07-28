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

// TestResolveFunctionTarget checks a target is accepted as a bare string or a
// FunctionRef (value or pointer), and that a ref surfaces its namespace.
func TestResolveFunctionTarget(t *testing.T) {
	if id, ns, err := resolveFunctionTarget("state::get"); err != nil || id != "state::get" || ns != "" {
		t.Errorf("string target = (%q,%q,%v), want (state::get,,nil)", id, ns, err)
	}
	if id, ns, err := resolveFunctionTarget(FunctionRef{ID: "run", Namespace: "agent"}); err != nil || id != "run" || ns != "agent" {
		t.Errorf("FunctionRef target = (%q,%q,%v), want (run,agent,nil)", id, ns, err)
	}
	if id, ns, err := resolveFunctionTarget(&FunctionRef{ID: "run", Namespace: "agent"}); err != nil || id != "run" || ns != "agent" {
		t.Errorf("*FunctionRef target = (%q,%q,%v), want (run,agent,nil)", id, ns, err)
	}
	if _, _, err := resolveFunctionTarget(42); err == nil {
		t.Error("resolveFunctionTarget(int) should error")
	}
	if _, _, err := resolveFunctionTarget((*FunctionRef)(nil)); err == nil {
		t.Error("resolveFunctionTarget(nil *FunctionRef) should error")
	}
}

// TestResolveTargetNamespace pins the precedence: explicit > ref's namespace > own.
func TestResolveTargetNamespace(t *testing.T) {
	cases := []struct {
		explicit, ref, own, want string
	}{
		{"explicit", "ref", "own", "explicit"},
		{"", "ref", "own", "ref"},
		{"", "", "own", "own"},
		{"", "", "", ""},
	}
	for _, tc := range cases {
		if got := resolveTargetNamespace(tc.explicit, tc.ref, tc.own); got != tc.want {
			t.Errorf("resolveTargetNamespace(%q,%q,%q) = %q, want %q", tc.explicit, tc.ref, tc.own, got, tc.want)
		}
	}
}

// TestRegisterFunctionRefCarriesNamespace verifies the returned ref carries the id and
// the worker's namespace at registration, so it can route triggers/invocations back.
func TestRegisterFunctionRefCarriesNamespace(t *testing.T) {
	c := New("ws://localhost:1", WithNamespace("orders"))
	ref, err := c.RegisterFunction("run", func(context.Context, json.RawMessage) (any, error) { return nil, nil })
	if err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}
	if ref.ID != "run" || ref.Namespace != "orders" {
		t.Errorf("ref = %+v, want {ID:run Namespace:orders}", ref)
	}
}

// TestUseNamespaceReturnsReceiverForOwnNamespace verifies a view request for the worker's
// own (normalized) namespace returns the receiver rather than a new connection.
func TestUseNamespaceReturnsReceiverForOwnNamespace(t *testing.T) {
	c := New("ws://localhost:1", WithNamespace("orders"))
	if got := c.UseNamespace("orders"); got != c {
		t.Error("UseNamespace(own) should return the receiver")
	}
	// An empty-namespace worker already lives in default.
	d := New("ws://localhost:1")
	if got := d.UseNamespace("default"); got != d {
		t.Error("UseNamespace(default) on a default worker should return the receiver")
	}
}

// TestUseNamespaceCachesViews verifies views are cached per namespace, keep the parent's
// name, and are torn down by the parent's Close.
func TestUseNamespaceCachesViews(t *testing.T) {
	c := New("ws://localhost:1", WithName("w"))
	t.Cleanup(func() { _ = c.Close() })

	v1 := c.UseNamespace("orders")
	v2 := c.UseNamespace("orders")
	if v1 != v2 {
		t.Error("UseNamespace should cache one view per namespace")
	}
	if v1 == c {
		t.Error("a view for a different namespace must be a distinct client")
	}
	if v1.namespace != "orders" || v1.name != "w" {
		t.Errorf("view namespace/name = %q/%q, want orders/w", v1.namespace, v1.name)
	}

	if err := c.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}
	select {
	case <-v1.shutdown:
		// view torn down with the parent
	default:
		t.Error("parent Close should close cached views")
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
