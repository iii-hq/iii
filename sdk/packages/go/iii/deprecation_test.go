package iii

import (
	"context"
	"encoding/json"
	"testing"
	"time"
)

func resetDeprecations() {
	deprecationMu.Lock()
	deprecationWarned = map[string]bool{}
	deprecationMu.Unlock()
}

// CONTRACT §5: RegisterTrigger and RegisterTriggerType keep working and log one warning
// per process per API, with the migration notice.
func TestDeprecatedTriggerAPIsWarnOncePerProcessAndKeepWorking(t *testing.T) {
	resetDeprecations()
	t.Cleanup(resetDeprecations)
	logs := captureLog(t)
	m := newMockEngine(t)
	c := connectClient(t, m)
	other := connectClient(t, m)
	handler := &stubTriggerHandler{registered: make(chan TriggerConfig, 4), unregistered: make(chan TriggerConfig, 4)}

	if _, err := c.RegisterTriggerType(RegisterTriggerTypeInput{ID: "webhook", Description: "Webhooks"}, handler); err != nil {
		t.Fatal(err)
	}
	_, _ = c.RegisterTriggerType(RegisterTriggerTypeInput{ID: "webhook-2", Description: "Webhooks 2"}, handler)
	_, _ = c.RegisterTrigger(RegisterTriggerInput{ID: "t-a", Type: "webhook", FunctionID: "hooks::in", Config: json.RawMessage(`{"path":"/a"}`)})
	_ = c.RegisterTriggerNamespaced("t-b", "webhook", "hooks::in", "ops", json.RawMessage(`{"path":"/b"}`))
	_, _ = other.RegisterTrigger(RegisterTriggerInput{ID: "t-c", Type: "webhook", FunctionID: "hooks::in"})
	_, _ = other.RegisterTriggerType(RegisterTriggerTypeInput{ID: "webhook-3", Description: ""}, handler)

	const trigger = "RegisterTrigger is deprecated and will be removed in a future release. Use client.Observe(source, target) instead. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x"
	const triggerType = "RegisterTriggerType is deprecated and will be removed in a future release. Register a function with an observe descriptor (RegisterFunction(id, handler, iii.RegisterFunctionOptions{Observe: &iii.ObserveHooks{ ... }})) instead. Migration guide: https://iii.dev/docs/upgrading/from-0-24-x"
	if n := logs.count(trigger); n != 1 {
		t.Errorf("RegisterTrigger warned %d times:\n%s", n, logs.String())
	}
	if n := logs.count(triggerType); n != 1 {
		t.Errorf("RegisterTriggerType warned %d times:\n%s", n, logs.String())
	}

	// still working: frames go out, the provider callbacks still run
	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return countType(msgs, string(MsgRegisterTriggerType)) == 3 && countType(msgs, string(MsgRegisterTrigger)) == 3
	}, 2*time.Second)
	if countType(got, string(MsgRegisterTriggerType)) != 3 || countType(got, string(MsgRegisterTrigger)) != 3 {
		t.Fatalf("frames: %d trigger types, %d triggers", countType(got, string(MsgRegisterTriggerType)), countType(got, string(MsgRegisterTrigger)))
	}
	c.handleRegisterTrigger(context.Background(), &RegisterTriggerMessage{ID: "x", TriggerType: "webhook", FunctionID: "hooks::in", Config: json.RawMessage(`{}`)})
	select {
	case cfg := <-handler.registered:
		if cfg.ID != "x" {
			t.Errorf("config %+v", cfg)
		}
	case <-time.After(2 * time.Second):
		t.Error("the trigger handler did not run")
	}
}
