package iii

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
)

func TestChannelURL(t *testing.T) {
	tests := []struct {
		name string
		base string
		id   string
		key  string
		dir  ChannelDirection
		want string
	}{
		{
			name: "write url",
			base: "ws://localhost:49134",
			id:   "abc123",
			key:  "11111111-1111-1111-1111-111111111111",
			dir:  ChannelWrite,
			want: "ws://localhost:49134/ws/channels/abc123?key=11111111-1111-1111-1111-111111111111&dir=write",
		},
		{
			name: "read url strips a trailing slash on the base",
			base: "ws://localhost:49134/",
			id:   "xyz",
			key:  "k",
			dir:  ChannelRead,
			want: "ws://localhost:49134/ws/channels/xyz?key=k&dir=read",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := channelURL(tt.base, tt.id, tt.key, tt.dir); got != tt.want {
				t.Errorf("channelURL = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestStreamChannelRefRoundtrip(t *testing.T) {
	// The wire fields are lowercase channel_id/access_key/direction; direction is a
	// lowercase enum.
	frame := `{"channel_id":"c1","access_key":"k1","direction":"read"}`
	var ref StreamChannelRef
	if err := json.Unmarshal([]byte(frame), &ref); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if ref.ChannelID != "c1" || ref.AccessKey != "k1" || ref.Direction != ChannelRead {
		t.Fatalf("decoded %+v", ref)
	}
	out, _ := json.Marshal(ref)
	if string(out) != frame {
		t.Errorf("re-marshal = %s, want %s", out, frame)
	}
}

func TestExtractChannelRefs(t *testing.T) {
	// A ref nested under "reader", another inside an array, and a non-ref object that
	// must be ignored.
	payload := json.RawMessage(`{
		"label": "batch",
		"reader": {"channel_id":"c1","access_key":"k1","direction":"read"},
		"nested": {"items": [
			{"channel_id":"c2","access_key":"k2","direction":"write"},
			{"not":"a ref"}
		]}
	}`)
	refs, err := ExtractChannelRefs(payload)
	if err != nil {
		t.Fatalf("ExtractChannelRefs: %v", err)
	}
	if len(refs) != 2 {
		t.Fatalf("found %d refs, want 2: %+v", len(refs), refs)
	}
	if r := refs["reader"]; r.ChannelID != "c1" || r.Direction != ChannelRead {
		t.Errorf("reader ref = %+v", r)
	}
	if r := refs["nested.items.0"]; r.ChannelID != "c2" || r.Direction != ChannelWrite {
		t.Errorf("nested ref = %+v", r)
	}
}

func TestAsChannelRefRejectsPartial(t *testing.T) {
	// Missing direction => not a ref.
	if _, ok := asChannelRef(map[string]any{"channel_id": "c", "access_key": "k"}); ok {
		t.Error("partial object must not be treated as a channel ref")
	}
}

// Close on a never-connected reader/writer is a no-op and must not error (the sockets
// connect lazily, so an unused channel end has no connection to close).
func TestChannelCloseBeforeConnect(t *testing.T) {
	r := OpenReader("ws://localhost:49134", StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelRead})
	if err := r.Close(); err != nil {
		t.Errorf("reader Close before connect = %v, want nil", err)
	}
	w := OpenWriter("ws://localhost:49134", StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelWrite})
	if err := w.Close(); err != nil {
		t.Errorf("writer Close before connect = %v, want nil", err)
	}
}

// TestChannelWriterMessageOpcode verifies SendMessage emits a TEXT frame (the message
// half of the protocol), by echoing it back through a server that reports the opcode.
func TestChannelWriterMessageOpcode(t *testing.T) {
	type framed struct {
		typ  websocket.MessageType
		data string
	}
	got := make(chan framed, 1)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		typ, data, err := conn.Read(r.Context())
		if err != nil {
			return
		}
		got <- framed{typ, string(data)}
		// Keep reading so the writer's closing handshake completes promptly instead of
		// timing out; a real engine likewise drains until the close frame.
		for {
			if _, _, err := conn.Read(r.Context()); err != nil {
				return
			}
		}
	}))
	defer srv.Close()

	base := "ws" + strings.TrimPrefix(srv.URL, "http")
	w := OpenWriter(base, StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelWrite})

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := w.SendMessage(ctx, "ping"); err != nil {
		t.Fatalf("SendMessage: %v", err)
	}
	t.Cleanup(func() { _ = w.Close() })

	select {
	case f := <-got:
		if f.typ != websocket.MessageText {
			t.Errorf("SendMessage opcode = %v, want text", f.typ)
		}
		if f.data != "ping" {
			t.Errorf("SendMessage payload = %q, want ping", f.data)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("server received no frame")
	}
}

// TestChannelReaderReadAll verifies the reader concatenates binary chunks and stops at
// the writer's close, while dispatching interleaved text frames to OnMessage.
func TestChannelReaderReadAll(t *testing.T) {
	// A server that, on connect, sends a text message, two binary chunks, then closes.
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		ctx := r.Context()
		_ = conn.Write(ctx, websocket.MessageText, []byte(`{"type":"progress"}`))
		_ = conn.Write(ctx, websocket.MessageBinary, []byte("hello "))
		_ = conn.Write(ctx, websocket.MessageBinary, []byte("world"))
		_ = conn.Close(websocket.StatusNormalClosure, "stream_complete")
	}))
	defer srv.Close()

	base := "ws" + strings.TrimPrefix(srv.URL, "http")
	reader := OpenReader(base, StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelRead})

	var (
		mu       sync.Mutex
		messages []string
	)
	reader.OnMessage(func(m string) {
		mu.Lock()
		messages = append(messages, m)
		mu.Unlock()
	})

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	got, err := reader.ReadAll(ctx)
	if err != nil {
		t.Fatalf("ReadAll: %v", err)
	}
	if string(got) != "hello world" {
		t.Errorf("ReadAll = %q, want %q", got, "hello world")
	}
	mu.Lock()
	defer mu.Unlock()
	if !reflect.DeepEqual(messages, []string{`{"type":"progress"}`}) {
		t.Errorf("messages = %v, want one progress message", messages)
	}
}

// TestChannelWriterChunking checks that a Write larger than channelFrameSize is split
// into multiple binary frames, by counting frames the server receives.
func TestChannelWriterChunking(t *testing.T) {
	var (
		mu        sync.Mutex
		binFrames int
		total     int
	)
	done := make(chan struct{})
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		conn.SetReadLimit(-1) // accept 64 KiB channel frames (default limit is 32 KiB)
		ctx := r.Context()
		for {
			typ, data, err := conn.Read(ctx)
			if err != nil {
				close(done)
				return
			}
			if typ == websocket.MessageBinary {
				mu.Lock()
				binFrames++
				total += len(data)
				mu.Unlock()
			}
		}
	}))
	defer srv.Close()

	base := "ws" + strings.TrimPrefix(srv.URL, "http")
	w := OpenWriter(base, StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelWrite})

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	payload := make([]byte, channelFrameSize*2+1) // spans 3 frames
	if err := w.Write(ctx, payload); err != nil {
		t.Fatalf("Write: %v", err)
	}
	_ = w.Close() // triggers the server's read error -> closes done

	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("server did not finish reading")
	}

	mu.Lock()
	defer mu.Unlock()
	if binFrames != 3 {
		t.Errorf("binary frames = %d, want 3 (64KiB chunks of 2*64KiB+1)", binFrames)
	}
	if total != len(payload) {
		t.Errorf("total bytes = %d, want %d", total, len(payload))
	}
}
