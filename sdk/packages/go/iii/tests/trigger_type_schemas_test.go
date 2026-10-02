//go:build integration

package iii_test

import (
	"context"
	"encoding/json"
	"reflect"
	"strconv"
	"testing"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

type triggerSchemaHandler struct{}

func (triggerSchemaHandler) RegisterTrigger(context.Context, iii.TriggerConfig) error { return nil }
func (triggerSchemaHandler) UnregisterTrigger(context.Context, iii.TriggerConfig) error {
	return nil
}

func TestTriggerTypeSchemasDiscovery(t *testing.T) {
	namespace := "test-go-trigger-schemas-" + strconv.FormatInt(time.Now().UnixNano(), 36)
	c := iii.New(engineWSURL(), iii.WithNamespace(namespace), iii.WithName(namespace))
	if err := c.Connect(ctxFor(t, 10*time.Second)); err != nil {
		t.Fatalf("connect to engine: %v", err)
	}
	t.Cleanup(func() { _ = c.Close() })

	configurationSchema := json.RawMessage(`{
		"type":"object",
		"properties":{"kinds":{"type":"array","items":{"enum":["message.received","message.status"]}}},
		"additionalProperties":false
	}`)
	requestSchema := json.RawMessage(`{
		"type":"object",
		"properties":{"kind":{"type":"string"},"message_id":{"type":"string"}},
		"required":["kind","message_id"]
	}`)

	for _, tt := range []struct {
		name    string
		options []iii.RegisterTriggerTypeOptions
	}{
		{
			name: "with_schemas",
			options: []iii.RegisterTriggerTypeOptions{{
				TriggerRequestFormat: configurationSchema,
				CallRequestFormat:    requestSchema,
			}},
		},
		{name: "legacy_without_schemas"},
	} {
		t.Run(tt.name, func(t *testing.T) {
			id := "test::go::trigger_schemas::" + tt.name
			const description = "Custom message events with optional discovery schemas"
			if err := c.RegisterTriggerType(id, description, triggerSchemaHandler{}, tt.options...); err != nil {
				t.Fatalf("RegisterTriggerType: %v", err)
			}

			query, err := json.Marshal(map[string]string{"id": id, "namespace": namespace})
			if err != nil {
				t.Fatal(err)
			}
			ctx := ctxFor(t, 5*time.Second)
			ticker := time.NewTicker(25 * time.Millisecond)
			defer ticker.Stop()
			var raw json.RawMessage
			for {
				raw, err = c.Trigger(ctx, iii.TriggerRequest{FunctionID: iii.FnInfoTriggers, Data: query})
				if err == nil {
					break
				}
				select {
				case <-ctx.Done():
					t.Fatalf("trigger type did not become discoverable: %v (last error: %v)", ctx.Err(), err)
				case <-ticker.C:
				}
			}

			var detail map[string]any
			if err := json.Unmarshal(raw, &detail); err != nil {
				t.Fatalf("decode trigger detail: %v\nraw: %s", err, raw)
			}
			for field, want := range map[string]string{
				"id": id, "namespace": namespace, "description": description,
			} {
				if detail[field] != want {
					t.Errorf("%s = %v, want %q", field, detail[field], want)
				}
			}
			for field, schema := range map[string]json.RawMessage{
				"configuration_schema": configurationSchema,
				"request_schema":       requestSchema,
			} {
				got, present := detail[field]
				if len(tt.options) == 0 {
					if present {
						t.Errorf("legacy registration unexpectedly exposes %s: %v", field, got)
					}
					continue
				}
				var want any
				if err := json.Unmarshal(schema, &want); err != nil {
					t.Fatal(err)
				}
				if !reflect.DeepEqual(got, want) {
					t.Errorf("%s = %v, want %v", field, got, want)
				}
			}
		})
	}
}
