package iii

import (
	"context"
	"crypto/sha1"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
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
		{
			name: "a query on the base comes before the channel's own",
			base: "ws://localhost:49134/?token=abc",
			id:   "xyz",
			key:  "k",
			dir:  ChannelRead,
			want: "ws://localhost:49134/ws/channels/xyz?token=abc&key=k&dir=read",
		},
		{
			name: "a base with a path and several query params keeps both",
			base: "ws://localhost:49134/engine/?token=abc&x=1",
			id:   "xyz",
			key:  "k",
			dir:  ChannelWrite,
			want: "ws://localhost:49134/engine/ws/channels/xyz?token=abc&x=1&key=k&dir=write",
		},
		{
			name: "an empty query on the base adds nothing",
			base: "ws://localhost:49134/?",
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

// TestChannelReaderEndOfStream checks how the reader reads the end of its socket: 1000 or
// a close with no code (1005, the engine's clean end) is EOF, while the engine's 1011
// abort or a drop without a close frame is an error. The bytes before it come back either
// way.
func TestChannelReaderEndOfStream(t *testing.T) {
	const reason = "channel writer aborted: connection lost before close"
	// wantCode is the close code the error carries, -1 when it is not a close.
	tests := []struct {
		name     string
		end      func(*websocket.Conn)
		wantErr  bool
		wantCode websocket.StatusCode
	}{
		{
			name: "1000 is end of stream",
			end:  func(c *websocket.Conn) { _ = c.Close(websocket.StatusNormalClosure, "stream_complete") },
		},
		{
			name: "a close with no code (1005) is end of stream",
			end:  func(c *websocket.Conn) { _ = c.Close(websocket.StatusNoStatusRcvd, "") },
		},
		{
			name:     "1011 is an error with its code and reason",
			end:      func(c *websocket.Conn) { _ = c.Close(websocket.StatusInternalError, reason) },
			wantErr:  true,
			wantCode: websocket.StatusInternalError,
		},
		{
			name:     "a drop without a close frame is an error",
			end:      func(c *websocket.Conn) { _ = c.CloseNow() },
			wantErr:  true,
			wantCode: -1,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				conn, err := websocket.Accept(w, r, nil)
				if err != nil {
					return
				}
				_ = conn.Write(r.Context(), websocket.MessageBinary, []byte("partial"))
				tt.end(conn)
			}))
			defer srv.Close()

			base := "ws" + strings.TrimPrefix(srv.URL, "http")
			reader := OpenReader(base, StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelRead})
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			got, err := reader.ReadAll(ctx)
			if string(got) != "partial" {
				t.Errorf("ReadAll = %q, want %q", got, "partial")
			}
			if !tt.wantErr {
				if err != nil {
					t.Fatalf("ReadAll error = %v, want nil (end of stream)", err)
				}
				return
			}
			if err == nil {
				t.Fatal("ReadAll error = nil, want an error")
			}
			if code := websocket.CloseStatus(err); code != tt.wantCode {
				t.Errorf("CloseStatus = %v, want %v", code, tt.wantCode)
			}
			var ce websocket.CloseError
			if errors.As(err, &ce) && ce.Reason != reason {
				t.Errorf("close reason = %q, want %q", ce.Reason, reason)
			}
		})
	}
}

// TestChannelReaderLocalClose checks that a read cut short by the reader's own Close, and
// a read after it, end as end of stream. The server drops the socket on the reader's close
// frame without a reply, as the engine does once its reader leaves.
func TestChannelReaderLocalClose(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		sum := sha1.Sum([]byte(r.Header.Get("Sec-WebSocket-Key") + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"))
		c, rw, err := w.(http.Hijacker).Hijack()
		if err != nil {
			return
		}
		defer c.Close()
		fmt.Fprintf(rw, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: %s\r\n\r\n", base64.StdEncoding.EncodeToString(sum[:]))
		_, _ = rw.WriteString("\x81\x02hi") // one unmasked text frame
		_ = rw.Flush()
		_, _ = rw.ReadByte() // the reader's close frame
	}))
	defer srv.Close()

	base := "ws" + strings.TrimPrefix(srv.URL, "http")
	reader := OpenReader(base, StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelRead})
	seen := make(chan struct{}, 1)
	reader.OnMessage(func(string) { seen <- struct{}{} })
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	type result struct {
		ok  bool
		err error
	}
	done := make(chan result, 1)
	go func() {
		_, ok, err := reader.NextBinary(ctx)
		done <- result{ok, err}
	}()
	// Close only once the read has the text frame: it is then past the dial, so its next
	// conn.Read fails with no close code and only the closed check makes that end of stream.
	select {
	case <-seen:
	case res := <-done:
		t.Fatalf("NextBinary before the text frame = (%v, %v), want it still reading", res.ok, res.err)
	case <-time.After(2 * time.Second):
		t.Fatal("reader got no text frame")
	}
	_ = reader.Close() // no reply comes, so Close itself reports the dropped socket
	select {
	case res := <-done:
		if res.ok || res.err != nil {
			t.Fatalf("NextBinary cut short by Close = (%v, %v), want end of stream", res.ok, res.err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("NextBinary cut short by Close did not return")
	}
	if _, ok, err := reader.NextBinary(ctx); ok || err != nil {
		t.Errorf("NextBinary after Close = (%v, %v), want end of stream without a redial", ok, err)
	}
}

// TestChannelWriterCloseAfterFailedWrite checks that Close after a failed Write ends the
// socket without a close frame, which the engine reads as an abort, so the reader cannot
// take the cut stream for a whole one.
func TestChannelWriterCloseAfterFailedWrite(t *testing.T) {
	closeCode := make(chan websocket.StatusCode, 1)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		for {
			if _, _, err := conn.Read(r.Context()); err != nil {
				closeCode <- websocket.CloseStatus(err)
				return
			}
		}
	}))
	defer srv.Close()

	base := "ws" + strings.TrimPrefix(srv.URL, "http")
	w := OpenWriter(base, StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelWrite})
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := w.Write(ctx, []byte("sent")); err != nil {
		t.Fatalf("Write: %v", err)
	}
	// Hold the message lock so the next Write fails on its deadline with the socket still open.
	if _, err := w.conn.Writer(ctx, websocket.MessageBinary); err != nil {
		t.Fatalf("Writer: %v", err)
	}
	short, cancelShort := context.WithTimeout(ctx, 10*time.Millisecond)
	defer cancelShort()
	if err := w.Write(short, []byte("lost")); err == nil {
		t.Fatal("Write = nil, want a deadline error")
	}
	if err := w.Close(); err != nil {
		t.Errorf("Close = %v, want nil", err)
	}
	select {
	case code := <-closeCode:
		if code != -1 {
			t.Errorf("writer close code = %v, want none (-1): a clean close here hides the lost bytes", code)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("server did not see the writer end")
	}
}

// TestChannelWriterCloseAfterFailedMessage checks the same after a failed SendMessage: a
// lost text frame is a cut stream too, so Close must not send 1000.
func TestChannelWriterCloseAfterFailedMessage(t *testing.T) {
	closeCode := make(chan websocket.StatusCode, 1)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		for {
			if _, _, err := conn.Read(r.Context()); err != nil {
				closeCode <- websocket.CloseStatus(err)
				return
			}
		}
	}))
	defer srv.Close()

	base := "ws" + strings.TrimPrefix(srv.URL, "http")
	w := OpenWriter(base, StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelWrite})
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := w.SendMessage(ctx, "sent"); err != nil {
		t.Fatalf("SendMessage: %v", err)
	}
	// Hold the message lock so the next SendMessage fails on its deadline with the socket still open.
	if _, err := w.conn.Writer(ctx, websocket.MessageText); err != nil {
		t.Fatalf("Writer: %v", err)
	}
	short, cancelShort := context.WithTimeout(ctx, 10*time.Millisecond)
	defer cancelShort()
	if err := w.SendMessage(short, "lost"); err == nil {
		t.Fatal("SendMessage = nil, want a deadline error")
	}
	if err := w.Close(); err != nil {
		t.Errorf("Close = %v, want nil", err)
	}
	select {
	case code := <-closeCode:
		if code != -1 {
			t.Errorf("writer close code = %v, want none (-1): a clean close here hides the lost message", code)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("server did not see the writer end")
	}
}

// TestChannelWriterChunking checks that a Write larger than channelFrameSize is split
// into multiple binary frames, by counting frames the server receives, and that Close
// ends the socket with 1000, the code the engine reads as a completed stream.
func TestChannelWriterChunking(t *testing.T) {
	var (
		mu        sync.Mutex
		binFrames int
		total     int
		closeCode websocket.StatusCode
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
				closeCode = websocket.CloseStatus(err)
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
	_ = w.Close() // makes the server's read fail -> closes done

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
	if closeCode != websocket.StatusNormalClosure {
		t.Errorf("writer close code = %v, want 1000 (anything else reads as an abort)", closeCode)
	}
}
