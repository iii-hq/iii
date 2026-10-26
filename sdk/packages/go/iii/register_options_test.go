package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/coder/websocket"
)

// sortedJSON re-encodes raw with sorted object keys at every level and numbers kept
// verbatim, so two encodings of the same JSON compare equal as strings and any extra,
// missing or changed field shows up in the diff.
func sortedJSON(t *testing.T, raw []byte) string {
	t.Helper()
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		t.Fatalf("sortedJSON: %v in %s", err, raw)
	}
	out, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("sortedJSON: %v", err)
	}
	return string(out)
}

// frameJSON is the canonical JSON of a whole recorded frame.
func frameJSON(t *testing.T, msg map[string]json.RawMessage) string {
	t.Helper()
	raw, err := json.Marshal(msg)
	if err != nil {
		t.Fatalf("frameJSON: %v", err)
	}
	return sortedJSON(t, raw)
}

// waitFrame waits for the first recorded frame of msgType with id and returns it.
func waitFrame(t *testing.T, m *mockEngine, msgType MessageType, id string) map[string]json.RawMessage {
	t.Helper()
	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return countRegister(msgs, string(msgType), id) > 0
	}, 3*time.Second)
	frame := firstWhere(got, func(msg map[string]json.RawMessage) bool {
		return messageType(msg) == string(msgType) && messageID(msg) == id
	})
	if frame == nil {
		t.Fatalf("no %s frame for %q; got %d frames", msgType, id, len(got))
	}
	return frame
}

func noopHandler(context.Context, json.RawMessage) (any, error) { return nil, nil }

// A2: description, request_format and response_format travel on registerfunction as
// the Node SDK spreads RegisterFunctionOptions into the message
// (sdk/packages/node/iii/src/iii.ts registerFunction; iii-types.ts RegisterFunctionMessage).
func TestRegisterFunctionOptionsSendDescriptionAndFormats(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)

	err := c.RegisterFunction("orders::create", noopHandler, RegisterFunctionOptions{
		Description:    "Creates an order",
		RequestFormat:  json.RawMessage(`{"type": "object", "properties": {"item": {"type": "string"}}}`),
		ResponseFormat: json.RawMessage(`{"type":"object"}`),
		Metadata:       json.RawMessage(`{"team":"shop"}`),
	})
	if err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}

	const want = `{"description":"Creates an order","id":"orders::create","metadata":{"team":"shop"},` +
		`"request_format":{"properties":{"item":{"type":"string"}},"type":"object"},` +
		`"response_format":{"type":"object"},"type":"registerfunction"}`
	if got := frameJSON(t, waitFrame(t, m, MsgRegisterFunction, "orders::create")); got != want {
		t.Errorf("registerfunction frame\n got %s\nwant %s", got, want)
	}
}

// A2: the zero options keep the default registration shape, with no new keys.
func TestRegisterFunctionWithoutOptionsSendsOnlyID(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	if err := c.RegisterFunction("plain::fn", noopHandler); err != nil {
		t.Fatalf("RegisterFunction: %v", err)
	}
	const want = `{"id":"plain::fn","type":"registerfunction"}`
	if got := frameJSON(t, waitFrame(t, m, MsgRegisterFunction, "plain::fn")); got != want {
		t.Errorf("registerfunction frame\n got %s\nwant %s", got, want)
	}
}

// A2: an invalid format is refused and nothing is registered.
func TestRegisterFunctionRejectsInvalidFormats(t *testing.T) {
	c := New("ws://127.0.0.1:0")
	for name, opts := range map[string]RegisterFunctionOptions{
		"RequestFormat":  {RequestFormat: json.RawMessage(`{"type":`)},
		"ResponseFormat": {ResponseFormat: json.RawMessage(`not json`)},
	} {
		err := c.RegisterFunction("bad::fn", noopHandler, opts)
		if err == nil || !strings.Contains(err.Error(), name+" must contain valid JSON") {
			t.Errorf("%s: error = %v, want a valid JSON error", name, err)
		}
		if err := RegisterFunctionTyped(c, "bad::typed", func(context.Context, struct{}) (struct{}, error) {
			return struct{}{}, nil
		}, opts); err == nil {
			t.Errorf("%s: RegisterFunctionTyped accepted invalid JSON", name)
		}
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if len(c.functions) != 0 {
		t.Errorf("registered %d functions, want none", len(c.functions))
	}
}

type typedOrderRequest struct {
	Item string `json:"item"`
}

type typedOrderResult struct {
	ID string `json:"id"`
}

// A2: with RegisterFunctionTyped an explicit format replaces the inferred one, the other
// stays inferred, and Description passes through.
func TestRegisterFunctionTypedExplicitFormatsOverrideInferred(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	err := RegisterFunctionTyped(c, "orders::typed", func(context.Context, typedOrderRequest) (typedOrderResult, error) {
		return typedOrderResult{}, nil
	}, RegisterFunctionOptions{
		Description:   "Typed order",
		RequestFormat: json.RawMessage(`{"type":"object","title":"hand-written"}`),
	})
	if err != nil {
		t.Fatalf("RegisterFunctionTyped: %v", err)
	}
	frame := waitFrame(t, m, MsgRegisterFunction, "orders::typed")
	if got := stringField(frame, "description"); got != "Typed order" {
		t.Errorf("description = %q, want Typed order", got)
	}
	if got, want := sortedJSON(t, frame["request_format"]), `{"title":"hand-written","type":"object"}`; got != want {
		t.Errorf("request_format = %s, want %s", got, want)
	}
	inferred, err := InferSchema[typedOrderResult]()
	if err != nil {
		t.Fatal(err)
	}
	if got, want := sortedJSON(t, frame["response_format"]), sortedJSON(t, inferred); got != want {
		t.Errorf("response_format = %s, want the inferred %s", got, want)
	}
}

// A3: an HTTP-invoked function registers with an invocation field shaped like the Node
// SDK's (iii.ts registerFunction with an HttpInvocationConfig: method defaults to POST).
func TestRegisterHTTPFunctionFrame(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	timeout := uint64(5000)
	headers := map[string]string{"x-team": "shop"}
	err := c.RegisterHTTPFunction("ext::lambda", HTTPInvocationRef{
		URL:       "https://example.com/fn",
		TimeoutMs: &timeout,
		Headers:   headers,
		Auth:      BearerAuth("LAMBDA_TOKEN"),
	}, RegisterFunctionOptions{Description: "External lambda"})
	if err != nil {
		t.Fatalf("RegisterHTTPFunction: %v", err)
	}
	headers["x-team"] = "changed after registration"

	const want = `{"description":"External lambda","id":"ext::lambda",` +
		`"invocation":{"auth":{"token_key":"LAMBDA_TOKEN","type":"bearer"},"headers":{"x-team":"shop"},` +
		`"method":"POST","timeout_ms":5000,"url":"https://example.com/fn"},"type":"registerfunction"}`
	if got := frameJSON(t, waitFrame(t, m, MsgRegisterFunction, "ext::lambda")); got != want {
		t.Errorf("registerfunction frame\n got %s\nwant %s", got, want)
	}

	// The replay sends the registration as it was made.
	c.mu.Lock()
	replayed, err := MarshalMessage(c.functions["ext::lambda"].message)
	c.mu.Unlock()
	if err != nil {
		t.Fatal(err)
	}
	if got := sortedJSON(t, replayed); got != want {
		t.Errorf("replayed frame\n got %s\nwant %s", got, want)
	}
}

// A3: an explicit method is kept, and the minimal registration carries only url and
// method.
func TestRegisterHTTPFunctionKeepsExplicitMethod(t *testing.T) {
	m := newMockEngine(t)
	c := connectClient(t, m)
	if err := c.RegisterHTTPFunction("ext::get", HTTPInvocationRef{URL: "https://example.com/get", Method: "GET"}); err != nil {
		t.Fatalf("RegisterHTTPFunction: %v", err)
	}
	const want = `{"id":"ext::get","invocation":{"method":"GET","url":"https://example.com/get"},"type":"registerfunction"}`
	if got := frameJSON(t, waitFrame(t, m, MsgRegisterFunction, "ext::get")); got != want {
		t.Errorf("registerfunction frame\n got %s\nwant %s", got, want)
	}
}

// A3: the auth constructors produce the Node SDK's HttpAuthConfig variants
// (sdk/packages/node/helpers/src/http/index.ts), byte for byte.
func TestHTTPAuthConstructors(t *testing.T) {
	for _, tc := range []struct {
		name string
		got  json.RawMessage
		want string
	}{
		{"hmac", HMACAuth("HMAC_SECRET"), `{"type":"hmac","secret_key":"HMAC_SECRET"}`},
		{"bearer", BearerAuth("TOKEN"), `{"type":"bearer","token_key":"TOKEN"}`},
		{"api_key", APIKeyAuth("x-api-key", "API_KEY"), `{"type":"api_key","header":"x-api-key","value_key":"API_KEY"}`},
		{"escaping", BearerAuth(`a"b`), `{"type":"bearer","token_key":"a\"b"}`},
	} {
		if string(tc.got) != tc.want {
			t.Errorf("%s = %s, want %s", tc.name, tc.got, tc.want)
		}
	}
}

// A3: an empty URL or invalid auth JSON is refused and nothing is registered.
func TestRegisterHTTPFunctionValidates(t *testing.T) {
	c := New("ws://127.0.0.1:0")
	if err := c.RegisterHTTPFunction("ext::none", HTTPInvocationRef{URL: "  "}); err == nil || !strings.Contains(err.Error(), "URL is empty") {
		t.Errorf("empty URL: error = %v", err)
	}
	if err := c.RegisterHTTPFunction("ext::auth", HTTPInvocationRef{URL: "https://x", Auth: json.RawMessage(`{`)}); err == nil {
		t.Error("invalid auth JSON accepted")
	}
	if err := c.RegisterHTTPFunction("ext::fmt", HTTPInvocationRef{URL: "https://x"}, RegisterFunctionOptions{RequestFormat: json.RawMessage(`[`)}); err == nil {
		t.Error("invalid RequestFormat accepted")
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if len(c.functions) != 0 {
		t.Errorf("registered %d functions, want none", len(c.functions))
	}
}

// A3: the engine invoking an HTTP function on this worker gets function_not_found (it
// has no handler here), not a panic; and it never runs in-process.
func TestHTTPFunctionHasNoLocalHandler(t *testing.T) {
	m := newMockEngine(t)
	id := mustUUID(t, "34343434-3434-3434-3434-343434343434")
	m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
		if messageType(msg) == string(MsgRegisterFunction) && messageID(msg) == "ext::remote" {
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = m.send(ctx, conn, &InvokeFunctionMessage{InvocationID: &id, FunctionID: "ext::remote", Data: json.RawMessage(`{}`)})
		}
	}
	c := connectClient(t, m)
	if err := c.RegisterHTTPFunction("ext::remote", HTTPInvocationRef{URL: "https://example.com"}); err != nil {
		t.Fatal(err)
	}
	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return firstOfType(msgs, string(MsgInvocationResult)) != nil
	}, 3*time.Second)
	res := firstOfType(got, string(MsgInvocationResult))
	if res == nil {
		t.Fatal("no invocation result")
	}
	var body ErrorBody
	if err := json.Unmarshal(res["error"], &body); err != nil || body.Code != "function_not_found" {
		t.Errorf("error = %s, want function_not_found", res["error"])
	}
	c.mu.Lock()
	fn := c.functions["ext::remote"]
	c.mu.Unlock()
	if fn.observe.localDispatchOption {
		t.Error("an HTTP function must not be eligible for local dispatch")
	}
}
