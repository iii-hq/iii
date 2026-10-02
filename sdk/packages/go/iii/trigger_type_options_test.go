package iii

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

func waitTriggerTypeRegistration(t *testing.T, m *mockEngine, id string) map[string]json.RawMessage {
	t.Helper()
	match := func(msg map[string]json.RawMessage) bool {
		return messageType(msg) == string(MsgRegisterTriggerType) && messageID(msg) == id
	}
	frames := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstWhere(msgs, match) != nil
	}, 3*time.Second)
	frame := firstWhere(frames, match)
	if frame == nil {
		t.Fatalf("no registertriggertype frame for %q", id)
	}
	return frame
}

func TestRegisterTriggerTypeOptionsWireFormat(t *testing.T) {
	for _, tc := range []struct {
		name   string
		opts   []RegisterTriggerTypeOptions
		config string
		call   string
	}{
		{name: "legacy"},
		{name: "zero options", opts: []RegisterTriggerTypeOptions{{}}},
		{name: "empty slices", opts: []RegisterTriggerTypeOptions{{TriggerRequestFormat: json.RawMessage{}, CallRequestFormat: json.RawMessage{}}}},
		{name: "configuration only", opts: []RegisterTriggerTypeOptions{{TriggerRequestFormat: json.RawMessage(`{"type":"object"}`)}}, config: `{"type":"object"}`},
		{name: "payload only", opts: []RegisterTriggerTypeOptions{{CallRequestFormat: json.RawMessage(`{"type":"string"}`)}}, call: `{"type":"string"}`},
		{name: "boolean schemas", opts: []RegisterTriggerTypeOptions{{TriggerRequestFormat: json.RawMessage(`true`), CallRequestFormat: json.RawMessage(`false`)}}, config: `true`, call: `false`},
	} {
		t.Run(tc.name, func(t *testing.T) {
			m := newMockEngine(t)
			c := connectClient(t, m)
			if err := c.RegisterTriggerType("custom::events", "Custom events", &stubTriggerHandler{}, tc.opts...); err != nil {
				t.Fatal(err)
			}
			frame := waitTriggerTypeRegistration(t, m, "custom::events")
			if stringField(frame, "description") != "Custom events" {
				t.Fatalf("registration lost description: %v", frame)
			}
			for field, want := range map[string]string{"trigger_request_format": tc.config, "call_request_format": tc.call} {
				if want == "" {
					if _, exists := frame[field]; exists {
						t.Errorf("unset %s must be omitted", field)
					}
				} else {
					jsonEqual(t, frame[field], want)
				}
			}
		})
	}
}

func TestRegisterTriggerTypeSchemasSurviveReconnect(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url, WithReconnectConfig(ReconnectConfig{
		InitialDelay: 10 * time.Millisecond, MaxDelay: 50 * time.Millisecond,
		BackoffMultiplier: 2, MaxRetries: -1,
	}))
	t.Cleanup(func() { _ = c.Close() })
	const config = `{"type":"object","properties":{"topic":{"type":"string"}}}`
	const call = `{"type":"object","properties":{"message":{"type":"string"}}}`
	opts := RegisterTriggerTypeOptions{TriggerRequestFormat: json.RawMessage(config), CallRequestFormat: json.RawMessage(call)}
	if err := c.RegisterTriggerType("custom::events", "Custom events", &stubTriggerHandler{}, opts); err != nil {
		t.Fatal(err)
	}
	// The caller can reuse its buffers after registration. Replay must still
	// contain the original schemas, even when registered before Connect.
	clear(opts.TriggerRequestFormat)
	clear(opts.CallRequestFormat)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	for attempt := 0; attempt < 2; attempt++ {
		if attempt > 0 {
			m.clear()
			m.closeActiveConnection()
		}
		frame := waitTriggerTypeRegistration(t, m, "custom::events")
		jsonEqual(t, frame["trigger_request_format"], config)
		jsonEqual(t, frame["call_request_format"], call)
	}
	// Re-registering without options clears the old advertised schemas.
	m.clear()
	if err := c.RegisterTriggerType("custom::events", "Updated events", &stubTriggerHandler{}); err != nil {
		t.Fatal(err)
	}
	frame := waitTriggerTypeRegistration(t, m, "custom::events")
	raw, err := json.Marshal(frame)
	if err != nil {
		t.Fatal(err)
	}
	jsonEqual(t, raw, `{"type":"registertriggertype","id":"custom::events","description":"Updated events"}`)
}

func TestRegisterTriggerTypeRejectsInvalidOptions(t *testing.T) {
	m := newMockEngine(t)
	c := New(m.url)
	t.Cleanup(func() { _ = c.Close() })
	handler := &stubTriggerHandler{}
	if err := c.RegisterTriggerType("custom::events", "Original", handler, RegisterTriggerTypeOptions{TriggerRequestFormat: json.RawMessage(`true`)}); err != nil {
		t.Fatal(err)
	}
	for _, tc := range []struct {
		name    string
		handler TriggerHandler
		opts    []RegisterTriggerTypeOptions
		want    string
	}{
		{name: "nil handler", want: "handler is nil"},
		{name: "multiple options", handler: handler, opts: []RegisterTriggerTypeOptions{{}, {}}, want: "at most one RegisterTriggerTypeOptions"},
		{name: "invalid config", handler: handler, opts: []RegisterTriggerTypeOptions{{TriggerRequestFormat: json.RawMessage(`{`)}}, want: "trigger_request_format"},
		{name: "invalid payload", handler: handler, opts: []RegisterTriggerTypeOptions{{CallRequestFormat: json.RawMessage(`{`)}}, want: "call_request_format"},
		{name: "whitespace", handler: handler, opts: []RegisterTriggerTypeOptions{{TriggerRequestFormat: json.RawMessage(` `)}}, want: "trigger_request_format"},
		{name: "trailing JSON", handler: handler, opts: []RegisterTriggerTypeOptions{{CallRequestFormat: json.RawMessage(`{} {}`)}}, want: "call_request_format"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			err := c.RegisterTriggerType("custom::events", "Invalid replacement", tc.handler, tc.opts...)
			if err == nil || !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("RegisterTriggerType error = %v, want %q", err, tc.want)
			}
		})
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	frame := waitTriggerTypeRegistration(t, m, "custom::events")
	if stringField(frame, "description") != "Original" {
		t.Fatalf("invalid registration replaced the original: %v", frame)
	}
	jsonEqual(t, frame["trigger_request_format"], `true`)
}
