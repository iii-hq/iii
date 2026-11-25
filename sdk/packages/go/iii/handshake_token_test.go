package iii

import (
	"context"
	"encoding/json"
	"net/http"
	"os"
	"reflect"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/google/uuid"
)

func TestHandshakeAuthorization(t *testing.T) {
	tests := []struct {
		name     string
		token    *string // nil leaves III_TOKEN unset
		explicit http.Header
		want     []string
	}{
		{name: "env token becomes a bearer", token: ptr("secret"), want: []string{"Bearer secret"}},
		{name: "env token is trimmed", token: ptr(" secret\n"), want: []string{"Bearer secret"}},
		{name: "explicit header wins", token: ptr("secret"), explicit: http.Header{"Authorization": {"Bearer explicit"}}, want: []string{"Bearer explicit"}},
		{name: "explicit header wins whatever its case", token: ptr("secret"), explicit: http.Header{"authorization": {"Bearer explicit"}}, want: []string{"Bearer explicit"}},
		{name: "blank token adds nothing", token: ptr("  "), want: nil},
		{name: "unset token adds nothing", want: nil},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			setToken(t, tt.token)
			m := newMockEngine(t)
			connectClient(t, m, WithHeaders(tt.explicit))

			if got := m.handshakeAt(0).header.Values("Authorization"); !reflect.DeepEqual(got, tt.want) {
				t.Errorf("Authorization = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestHandshakeKeepsTheCallersHeaders(t *testing.T) {
	setToken(t, ptr("secret"))
	explicit := http.Header{"X-Other": {"kept"}}
	m := newMockEngine(t)
	connectClient(t, m, WithHeaders(explicit))

	if got := m.handshakeAt(0).header.Get("X-Other"); got != "kept" {
		t.Errorf("X-Other = %q, want kept", got)
	}
	if len(explicit) != 1 {
		t.Errorf("WithHeaders argument was modified: %v", explicit)
	}
}

// TestStreamsSendTheHandshakeHeaders covers both ways a stream gets its engine: the
// ends of a Pipe (the client's WithHeaders ride along, and the pipe is created in the
// default namespace) and a stream bound by hand to a client whose address carries a query.
func TestStreamsSendTheHandshakeHeaders(t *testing.T) {
	setToken(t, ptr("secret"))
	ref := func(dir ChannelDirection) StreamChannelRef {
		return StreamChannelRef{ChannelID: "chan", AccessKey: "key", Direction: dir}
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	t.Run("Pipe", func(t *testing.T) {
		m := newMockEngine(t)
		namespace := make(chan string, 1)
		m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
			var id uuid.UUID
			if stringField(msg, "function_id") != FnCreateChannel || json.Unmarshal(msg["invocation_id"], &id) != nil {
				return
			}
			namespace <- stringField(msg, "namespace")
			result, _ := json.Marshal(map[string]StreamChannelRef{"writer": ref(ChannelWrite), "reader": ref(ChannelRead)})
			_ = m.send(ctx, conn, &InvocationResultMessage{InvocationID: id, FunctionID: FnCreateChannel, Result: result})
		}
		c := connectClient(t, m, WithHeaders(http.Header{"X-Other": {"kept"}}), WithNamespace("orders"))

		in, out, err := c.Pipe(ctx)
		if err != nil {
			t.Fatalf("Pipe: %v", err)
		}
		if ns := <-namespace; ns != "default" {
			t.Errorf("engine::channels::create namespace = %q, want default", ns)
		}
		if err := out.Send(ctx, "hi"); err != nil {
			t.Fatalf("Send: %v", err)
		}
		t.Cleanup(func() { _ = out.Close() })
		readCtx, cancelRead := context.WithTimeout(ctx, 200*time.Millisecond)
		defer cancelRead()
		_, _ = in.ReadAll(readCtx)

		for i, dir := range []string{"write", "read"} {
			h := m.handshakeAt(i + 1) // 0 is the main connection
			if want := "/ws/channels/chan?key=key&dir=" + dir; h.uri != want {
				t.Errorf("%s uri = %q, want %q", dir, h.uri, want)
			}
			if got := h.header.Get("Authorization"); got != "Bearer secret" {
				t.Errorf("%s Authorization = %q, want Bearer secret", dir, got)
			}
			if got := h.header.Get("X-Other"); got != "kept" {
				t.Errorf("%s X-Other = %q, want kept", dir, got)
			}
		}
	})

	t.Run("Bind", func(t *testing.T) {
		m := newMockEngine(t)
		c := New(m.url + "/?token=abc")
		t.Cleanup(func() { _ = c.Close() })
		w := (&OutputStream{inner: newOutput(ptrRef(ref(ChannelWrite)), nil, nil)}).Bind(c)
		r := (&InputStream{inner: newInput(ptrRef(ref(ChannelRead)), nil, nil)}).Bind(c)
		if err := w.Send(ctx, "hi"); err != nil {
			t.Fatalf("Send: %v", err)
		}
		t.Cleanup(func() { _ = w.Close() })
		readCtx, cancelRead := context.WithTimeout(ctx, 200*time.Millisecond)
		defer cancelRead()
		_, _ = r.ReadAll(readCtx)

		for i, dir := range []string{"write", "read"} {
			h := m.handshakeAt(i)
			if want := "/ws/channels/chan?token=abc&key=key&dir=" + dir; h.uri != want {
				t.Errorf("%s uri = %q, want %q", dir, h.uri, want)
			}
			if got := h.header.Get("Authorization"); got != "Bearer secret" {
				t.Errorf("%s Authorization = %q, want Bearer secret", dir, got)
			}
		}
	})
}

func ptrRef(ref StreamChannelRef) *StreamChannelRef { return &ref }

func ptr(s string) *string { return &s }

// setToken sets III_TOKEN for the test, or removes it when token is nil.
func setToken(t *testing.T, token *string) {
	t.Helper()
	t.Setenv("III_TOKEN", "") // registers the restore of the original value
	if token == nil {
		_ = os.Unsetenv("III_TOKEN")
		return
	}
	t.Setenv("III_TOKEN", *token)
}
