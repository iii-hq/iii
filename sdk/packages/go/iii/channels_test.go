package iii

import (
	"context"
	"crypto/sha1"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
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

// readStreamAt is the stream to read ref, bound to the engine at base.
func readStreamAt(base string, ref StreamChannelRef) *InputStream {
	return &InputStream{inner: newInput(&ref, nil, &endpoint{address: base})}
}

// writeStreamAt is the stream to write ref, bound to the engine at base.
func writeStreamAt(base string, ref StreamChannelRef) *OutputStream {
	return &OutputStream{inner: newOutput(&ref, nil, &endpoint{address: base})}
}

func wsBase(srv *httptest.Server) string { return "ws" + strings.TrimPrefix(srv.URL, "http") }

// streamCode is the stream error code of err, "" for another error.
func streamCode(err error) string {
	var ie *InvocationError
	if errors.As(err, &ie) {
		return ie.Code
	}
	return ""
}

// TestVisitStreamsFindsNestedRefs: the walks find a ref nested in an object and one in
// an array, and ignore an object that is not a ref.
func TestVisitStreamsFindsNestedRefs(t *testing.T) {
	payload := json.RawMessage(`{
		"label": "batch",
		"reader": {"channel_id":"c1","access_key":"k1","direction":"read"},
		"nested": {"items": [
			{"channel_id":"c2","access_key":"k2","direction":"write"},
			{"not":"a ref"}
		]}
	}`)
	v, ok := decodeValue(payload)
	if !ok {
		t.Fatal("decodeValue failed")
	}
	got := map[string]ChannelDirection{}
	visitRefs(v, func(ref StreamChannelRef) { got[ref.ChannelID] = ref.Direction })
	if want := map[string]ChannelDirection{"c1": ChannelRead, "c2": ChannelWrite}; !reflect.DeepEqual(got, want) {
		t.Fatalf("refs = %v, want %v", got, want)
	}
	if !containsStream(payload) || !hasWriteStream(payload) {
		t.Error("containsStream / hasWriteStream missed a ref")
	}
	if containsStream(json.RawMessage(`{"channel_id":"c","nested":{"not":"a ref"}}`)) {
		t.Error("containsStream found a stream in a value without one")
	}
}

func TestRefOfRejectsPartial(t *testing.T) {
	// Missing direction => not a ref; an unknown direction neither.
	for _, v := range []map[string]any{
		{"channel_id": "c", "access_key": "k"},
		{"channel_id": "c", "access_key": "k", "direction": "both"},
		{"channel_id": "c", "direction": "read"},
	} {
		if _, ok := refOf(v); ok {
			t.Errorf("refOf(%v) = ok, want a partial object not to be a ref", v)
		}
	}
}

// TestStreamEndsBeforeConnect: a stream never read opens no socket, and a cancel of it
// opens none either (a read after it fails with STREAM_CLOSED, never a clean end);
// closing a stream never written opens its socket and closes it, so its reader sees an
// empty stream rather than waiting for one.
func TestStreamEndsBeforeConnect(t *testing.T) {
	var (
		mu    sync.Mutex
		dials []string
	)
	closed := make(chan websocket.StatusCode, 1)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		dials = append(dials, r.URL.Query().Get("dir"))
		mu.Unlock()
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		for {
			if _, _, err := conn.Read(r.Context()); err != nil {
				closed <- websocket.CloseStatus(err)
				return
			}
		}
	}))
	defer srv.Close()

	in := readStreamAt(wsBase(srv), StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelRead})
	if err := in.Close(); err != nil {
		t.Errorf("Close of an unread stream = %v, want nil", err)
	}
	if n, err := in.Read(make([]byte, 4)); n != 0 || streamCode(err) != CodeStreamClosed {
		t.Errorf("Read after Cancel = (%d, %v), want (0, STREAM_CLOSED)", n, err)
	}
	out := writeStreamAt(wsBase(srv), StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelWrite})
	if err := out.Close(); err != nil {
		t.Errorf("Close of an unwritten stream = %v, want nil", err)
	}
	select {
	case code := <-closed:
		if code != websocket.StatusNormalClosure {
			t.Errorf("close code = %v, want 1000", code)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("the unwritten stream was not opened and closed")
	}
	mu.Lock()
	defer mu.Unlock()
	if !reflect.DeepEqual(dials, []string{"write"}) {
		t.Errorf("dials = %v, want only the writer", dials)
	}
}

// TestOutputStreamSendOpcode verifies Send emits a TEXT frame holding the message as
// JSON (the message half of the protocol), by echoing it back through a server that
// reports the opcode.
func TestOutputStreamSendOpcode(t *testing.T) {
	type framed struct {
		typ  websocket.MessageType
		data string
	}
	got := make(chan framed, 2)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		for {
			typ, data, err := conn.Read(r.Context())
			if err != nil {
				return
			}
			got <- framed{typ, string(data)}
		}
	}))
	defer srv.Close()

	w := writeStreamAt(wsBase(srv), StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelWrite})
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := w.Send(ctx, map[string]string{"token": "ping"}); err != nil {
		t.Fatalf("Send: %v", err)
	}
	if err := w.Send(ctx, json.RawMessage(`"raw"`)); err != nil {
		t.Fatalf("Send raw: %v", err)
	}
	t.Cleanup(func() { _ = w.Close() })

	for _, want := range []string{`{"token":"ping"}`, `"raw"`} {
		select {
		case f := <-got:
			if f.typ != websocket.MessageText || f.data != want {
				t.Errorf("Send frame = %v %q, want text %q", f.typ, f.data, want)
			}
		case <-time.After(2 * time.Second):
			t.Fatal("server received no frame")
		}
	}
}

// TestInputStreamFramesAndReadAll verifies the frames come in order, bytes and
// messages told apart by opcode (a text frame that is not JSON reads as a JSON
// string), and that a byte read concatenates the binary frames up to the writer's
// close, skipping the messages.
func TestInputStreamFramesAndReadAll(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := websocket.Accept(w, r, nil)
		if err != nil {
			return
		}
		ctx := r.Context()
		_ = conn.Write(ctx, websocket.MessageText, []byte(`{"type":"progress"}`))
		_ = conn.Write(ctx, websocket.MessageBinary, []byte("hello "))
		_ = conn.Write(ctx, websocket.MessageText, []byte(`not json`))
		_ = conn.Write(ctx, websocket.MessageBinary, []byte("world"))
		_ = conn.Close(websocket.StatusNormalClosure, "stream_complete")
	}))
	defer srv.Close()
	ref := StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelRead}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	var frames []string
	for f, err := range readStreamAt(wsBase(srv), ref).Frames(ctx) {
		if err != nil {
			t.Fatalf("Frames: %v", err)
		}
		frames = append(frames, string(f.Type)+":"+string(f.Data))
	}
	want := []string{`text:{"type":"progress"}`, "binary:hello ", "text:not json", "binary:world"}
	if !reflect.DeepEqual(frames, want) {
		t.Errorf("frames = %q, want %q", frames, want)
	}

	var messages []string
	for m, err := range readStreamAt(wsBase(srv), ref).Messages(ctx) {
		if err != nil {
			t.Fatalf("Messages: %v", err)
		}
		messages = append(messages, string(m))
	}
	if want := []string{`{"type":"progress"}`, `"not json"`}; !reflect.DeepEqual(messages, want) {
		t.Errorf("messages = %q, want %q", messages, want)
	}

	got, err := readStreamAt(wsBase(srv), ref).ReadAll(ctx)
	if err != nil {
		t.Fatalf("ReadAll: %v", err)
	}
	if string(got) != "hello world" {
		t.Errorf("ReadAll = %q, want %q", got, "hello world")
	}
}

// TestInputStreamEndOfStream checks how a read reads the end of its socket: 1000 or a
// close with no code (1005, the engine's clean end) is the end, while the engine's 1011
// abort or a drop without a close frame is STREAM_ABORTED. The bytes before it come
// back either way.
func TestInputStreamEndOfStream(t *testing.T) {
	const reason = "channel writer aborted: connection lost before close"
	tests := []struct {
		name       string
		end        func(*websocket.Conn)
		wantReason string // "" for the end of the stream
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
			name:       "1011 is an abort with its reason",
			end:        func(c *websocket.Conn) { _ = c.Close(websocket.StatusInternalError, reason) },
			wantReason: reason,
		},
		{
			name:       "a drop without a close frame is an abort",
			end:        func(c *websocket.Conn) { _ = c.CloseNow() },
			wantReason: "connection lost",
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

			in := readStreamAt(wsBase(srv), StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelRead})
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			got, err := in.ReadAll(ctx)
			if string(got) != "partial" {
				t.Errorf("ReadAll = %q, want %q", got, "partial")
			}
			if tt.wantReason == "" {
				if err != nil {
					t.Fatalf("ReadAll error = %v, want nil (end of stream)", err)
				}
				return
			}
			var ie *InvocationError
			if !errors.As(err, &ie) || ie.Code != CodeStreamAborted || !strings.Contains(ie.Message, tt.wantReason) {
				t.Fatalf("ReadAll error = %v, want STREAM_ABORTED with %q", err, tt.wantReason)
			}
		})
	}
}

// TestInputStreamCancelDuringRead checks that a read cut short by Cancel, and every read
// after it, fail with STREAM_CLOSED: a cancelled stream never looks complete. The server
// drops the socket on the reader's close frame without a reply, as the engine does once
// its reader leaves.
func TestInputStreamCancelDuringRead(t *testing.T) {
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

	in := readStreamAt(wsBase(srv), StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelRead})
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	const reader = 7 // one reader for the reads before and after the cancel
	seen := make(chan struct{}, 1)
	done := make(chan error, 1)
	go func() {
		for {
			f, err := in.inner.next(ctx, reader)
			if err == io.EOF {
				done <- nil
				return
			}
			if err != nil {
				done <- err
				return
			}
			if f.Type == FrameText {
				seen <- struct{}{}
			}
		}
	}()
	// Cancel only once the read has the text frame: it is then past the dial, so its next
	// conn.Read fails with no close code and only the cancel makes that end of stream.
	select {
	case <-seen:
	case err := <-done:
		t.Fatalf("read before the text frame ended with %v, want it still reading", err)
	case <-time.After(2 * time.Second):
		t.Fatal("reader got no text frame")
	}
	in.Cancel()
	select {
	case err := <-done:
		if streamCode(err) != CodeStreamClosed || !strings.Contains(err.Error(), cancelledMessage) {
			t.Fatalf("read cut short by Cancel = %v, want STREAM_CLOSED (%s)", err, cancelledMessage)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("read cut short by Cancel did not return")
	}
	if _, err := in.inner.next(ctx, reader); streamCode(err) != CodeStreamClosed {
		t.Errorf("read after Cancel = %v, want STREAM_CLOSED without a redial", err)
	}
	// Any reader of a cancelled stream gets STREAM_CLOSED.
	if _, err := in.Read(make([]byte, 4)); streamCode(err) != CodeStreamClosed {
		t.Errorf("Read by another reader = %v, want STREAM_CLOSED", err)
	}
}

// TestOutputStreamFailedWriteAborts checks that a write that fails (here on its
// deadline, with the socket still open) drops the socket without a close frame, which
// the engine reads as an abort, so the reader cannot take the cut stream for a whole
// one; later writes fail with STREAM_CLOSED and Close returns nil. Same for a send.
func TestOutputStreamFailedWriteAborts(t *testing.T) {
	for _, kind := range []websocket.MessageType{websocket.MessageBinary, websocket.MessageText} {
		t.Run(kind.String(), func(t *testing.T) {
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

			out := writeStreamAt(wsBase(srv), StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelWrite})
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			send := func(ctx context.Context, data string) error {
				if kind == websocket.MessageText {
					return out.Send(ctx, data)
				}
				return out.Write(ctx, []byte(data))
			}
			if err := send(ctx, "sent"); err != nil {
				t.Fatalf("first write: %v", err)
			}
			// Hold the message lock so the next write fails on its deadline with the socket
			// still open.
			if _, err := out.inner.ws.conn.Writer(ctx, kind); err != nil {
				t.Fatalf("Writer: %v", err)
			}
			short, cancelShort := context.WithTimeout(ctx, 10*time.Millisecond)
			defer cancelShort()
			if err := send(short, "lost"); !errors.Is(err, context.DeadlineExceeded) {
				t.Fatalf("write = %v, want its deadline", err)
			}
			if err := send(ctx, "after"); streamCode(err) != CodeStreamClosed {
				t.Errorf("write after the failure = %v, want STREAM_CLOSED", err)
			}
			if err := out.Close(); err != nil {
				t.Errorf("Close = %v, want nil", err)
			}
			select {
			case code := <-closeCode:
				if code != -1 {
					t.Errorf("writer close code = %v, want none (-1): a clean close here hides the lost data", code)
				}
			case <-time.After(2 * time.Second):
				t.Fatal("server did not see the writer end")
			}
		})
	}
}

// TestOutputStreamChunking checks that a Write larger than channelFrameSize is split
// into multiple binary frames, by counting frames the server receives, and that Close
// ends the socket with 1000, the code the engine reads as a completed stream.
func TestOutputStreamChunking(t *testing.T) {
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

	w := writeStreamAt(wsBase(srv), StreamChannelRef{ChannelID: "c", AccessKey: "k", Direction: ChannelWrite})
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	payload := make([]byte, channelFrameSize*2+1) // spans 3 frames
	if err := w.Write(ctx, payload); err != nil {
		t.Fatalf("Write: %v", err)
	}
	if err := w.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}

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
	if err := w.Write(ctx, []byte("late")); streamCode(err) != CodeStreamClosed {
		t.Errorf("Write after Close = %v, want STREAM_CLOSED", err)
	}
}
