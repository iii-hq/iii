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

// TestChannelsSendTheHandshakeHeaders covers both ways to get a channel end: from
// CreateChannel (the client's WithHeaders ride along) and OpenReader/OpenWriter
// (III_TOKEN only).
func TestChannelsSendTheHandshakeHeaders(t *testing.T) {
	setToken(t, ptr("secret"))
	ref := func(dir ChannelDirection) StreamChannelRef {
		return StreamChannelRef{ChannelID: "chan", AccessKey: "key", Direction: dir}
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	t.Run("CreateChannel", func(t *testing.T) {
		m := newMockEngine(t)
		m.onReceive = func(conn *websocket.Conn, msg map[string]json.RawMessage) {
			var id uuid.UUID
			if stringField(msg, "function_id") != FnCreateChannel || json.Unmarshal(msg["invocation_id"], &id) != nil {
				return
			}
			result, _ := json.Marshal(map[string]StreamChannelRef{"writer": ref(ChannelWrite), "reader": ref(ChannelRead)})
			_ = m.send(ctx, conn, &InvocationResultMessage{InvocationID: id, FunctionID: FnCreateChannel, Result: result})
		}
		c := connectClient(t, m, WithHeaders(http.Header{"X-Other": {"kept"}}))

		ch, err := c.CreateChannel(ctx, nil)
		if err != nil {
			t.Fatalf("CreateChannel: %v", err)
		}
		t.Cleanup(func() { _ = ch.Writer.Close(); _ = ch.Reader.Close() })
		if err := ch.Writer.SendMessage(ctx, "hi"); err != nil {
			t.Fatalf("SendMessage: %v", err)
		}
		if _, err := ch.Reader.ensureConnected(ctx); err != nil {
			t.Fatalf("reader connect: %v", err)
		}

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

	t.Run("OpenWriter and OpenReader", func(t *testing.T) {
		m := newMockEngine(t)
		w := OpenWriter(m.url+"/?token=abc", ref(ChannelWrite))
		r := OpenReader(m.url+"/?token=abc", ref(ChannelRead))
		t.Cleanup(func() { _ = w.Close(); _ = r.Close() })
		if err := w.SendMessage(ctx, "hi"); err != nil {
			t.Fatalf("SendMessage: %v", err)
		}
		if _, err := r.ensureConnected(ctx); err != nil {
			t.Fatalf("reader connect: %v", err)
		}

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
