package iii

import (
	"bufio"
	"context"
	"crypto/sha1"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
)

type capturedWSFrame struct {
	fin     bool
	opcode  byte
	masked  bool
	payload []byte
}

type rawWSServer struct {
	listener net.Listener
	frames   chan capturedWSFrame
	err      chan error
}

func newRawWSServer(t *testing.T) *rawWSServer {
	t.Helper()
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	server := &rawWSServer{
		listener: listener,
		frames:   make(chan capturedWSFrame, 128),
		err:      make(chan error, 1),
	}
	t.Cleanup(func() { _ = listener.Close() })
	go server.serve()
	return server
}

func (s *rawWSServer) url() string { return "ws://" + s.listener.Addr().String() }

func (s *rawWSServer) serve() {
	conn, err := s.listener.Accept()
	if err != nil {
		s.err <- err
		return
	}
	defer conn.Close()
	_ = conn.SetReadDeadline(time.Now().Add(60 * time.Second))
	reader := bufio.NewReader(conn)
	request, err := http.ReadRequest(reader)
	if err != nil {
		s.err <- err
		return
	}
	key := request.Header.Get("Sec-WebSocket-Key")
	if key == "" {
		s.err <- fmt.Errorf("missing Sec-WebSocket-Key")
		return
	}
	digest := sha1.Sum([]byte(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"))
	_, err = fmt.Fprintf(conn, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: %s\r\n\r\n", base64.StdEncoding.EncodeToString(digest[:]))
	if err != nil {
		s.err <- err
		return
	}

	for {
		first, err := reader.ReadByte()
		if err != nil {
			if !errors.Is(err, net.ErrClosed) && !errors.Is(err, io.EOF) {
				s.err <- err
			}
			return
		}
		second, err := reader.ReadByte()
		if err != nil {
			s.err <- err
			return
		}
		length := uint64(second & 0x7f)
		if length == 126 {
			var extended uint16
			if err := binary.Read(reader, binary.BigEndian, &extended); err != nil {
				s.err <- err
				return
			}
			length = uint64(extended)
		} else if length == 127 {
			if err := binary.Read(reader, binary.BigEndian, &length); err != nil {
				s.err <- err
				return
			}
		}
		frame := capturedWSFrame{fin: first&0x80 != 0, opcode: first & 0x0f, masked: second&0x80 != 0}
		var mask [4]byte
		if frame.masked {
			if _, err := io.ReadFull(reader, mask[:]); err != nil {
				s.err <- err
				return
			}
		}
		limit := uint64(maxJSONFrameBytes)
		if frame.opcode == 0 {
			limit = uint64(maxJSONFragmentBytes)
		}
		if length > limit {
			s.err <- fmt.Errorf("frame payload length %d exceeds expected maximum %d", length, limit)
			return
		}
		frame.payload = make([]byte, int(length))
		if _, err := io.ReadFull(reader, frame.payload); err != nil {
			s.err <- err
			return
		}
		if frame.masked {
			for i := range frame.payload {
				frame.payload[i] ^= mask[i&3]
			}
		}
		s.frames <- frame
	}
}

func (s *rawWSServer) readFrames(t *testing.T, count int) []capturedWSFrame {
	t.Helper()
	frames := make([]capturedWSFrame, 0, count)
	timer := time.NewTimer(5 * time.Second)
	defer timer.Stop()
	for len(frames) < count {
		select {
		case frame := <-s.frames:
			frames = append(frames, frame)
		case err := <-s.err:
			t.Fatalf("raw WebSocket server: %v", err)
		case <-timer.C:
			t.Fatalf("timed out waiting for frame %d of %d; received %d frames: %v", len(frames)+1, count, len(frames), frameSummary(frames))
		}
	}
	return frames
}

func envelopeWithSize(target int, functionID string) []byte {
	prefix := []byte(fmt.Sprintf(`{"type":"invokefunction","function_id":%q,"data":"`, functionID))
	suffix := []byte(`"}`)
	return []byte(string(prefix) + strings.Repeat("x", target-len(prefix)-len(suffix)) + string(suffix))
}

func assertFrameMessage(t *testing.T, frames []capturedWSFrame, want []byte, fragmented bool) {
	t.Helper()
	if !fragmented {
		if len(frames) != 1 || !frames[0].fin || frames[0].opcode != 1 || !frames[0].masked {
			t.Fatalf("single-frame message has unexpected frames: %+v", frameSummary(frames))
		}
		if string(frames[0].payload) != string(want) {
			t.Fatal("single text frame payload differs from serialized envelope")
		}
		return
	}
	if len(frames) < 2 {
		t.Fatalf("fragmented message has %d frame(s)", len(frames))
	}
	var message []byte
	for i, frame := range frames {
		if len(frame.payload) > maxJSONFragmentBytes {
			t.Fatalf("frame %d has %d payload bytes", i, len(frame.payload))
		}
		if !frame.masked {
			t.Fatalf("client frame %d is not masked", i)
		}
		wantOpcode := byte(0)
		if i == 0 {
			wantOpcode = 1
		}
		if frame.opcode != wantOpcode {
			t.Fatalf("frame %d opcode = %d, want %d", i, frame.opcode, wantOpcode)
		}
		if frame.fin != (i == len(frames)-1) {
			t.Fatalf("frame %d FIN=%t, want %t", i, frame.fin, i == len(frames)-1)
		}
		message = append(message, frame.payload...)
	}
	if string(message) != string(want) {
		t.Fatalf("reassembled payload differs: got %d bytes, want %d", len(message), len(want))
	}
}

func frameSummary(frames []capturedWSFrame) []string {
	out := make([]string, len(frames))
	for i, frame := range frames {
		out[i] = fmt.Sprintf("FIN=%t opcode=%d payload=%d", frame.fin, frame.opcode, len(frame.payload))
	}
	return out
}

func TestJSONWebSocketFrameFragmentationAndBoundaries(t *testing.T) {
	server := newRawWSServer(t)
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	conn, _, err := websocket.Dial(ctx, server.url(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.CloseNow()
	client := New(server.url())

	for _, size := range []int{maxJSONFrameBytes, maxJSONFrameBytes + 1, 40 * 1024 * 1024, maxJSONMessageBytes} {
		frame := envelopeWithSize(size, "boundary")
		if !json.Valid(frame) {
			t.Fatalf("generated envelope of %d bytes is invalid JSON", size)
		}
		checked, err := prepareFrame(frame)
		if err != nil || len(checked) != size {
			t.Fatalf("prepareFrame(%d): len=%d err=%v", size, len(checked), err)
		}
		if err := client.writeJSON(ctx, conn, frame); err != nil {
			t.Fatalf("writeJSON(%d): %v", size, err)
		}
		fragmented := size > maxJSONFrameBytes
		frameCount := 1
		if fragmented {
			frameCount = (size+maxJSONFragmentBytes-1)/maxJSONFragmentBytes + 1 // Close writes the final FIN frame.
		}
		assertFrameMessage(t, server.readFrames(t, frameCount), frame, fragmented)
	}

	tooLarge := envelopeWithSize(maxJSONMessageBytes+1, "boundary")
	_, err = prepareFrame(tooLarge)
	var invocationErr *InvocationError
	if !errors.As(err, &invocationErr) || invocationErr.Code != "payload_too_large" {
		t.Fatalf("64 MiB + 1 byte error = %v", err)
	}
	wantError := "Serialized JSON envelope is 67108865 bytes; limit is 67108864 bytes. Use channels for larger payloads: https://iii.dev/docs/creating-workers/channels#payload-size"
	if invocationErr.Message != wantError {
		t.Fatalf("error message = %q, want %q", invocationErr.Message, wantError)
	}
}

func TestJSONWebSocketFragmentPreservesSplitUTF8(t *testing.T) {
	server := newRawWSServer(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	conn, _, err := websocket.Dial(ctx, server.url(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.CloseNow()

	const targetSize = maxJSONFrameBytes + 1
	prefix := []byte(`{"type":"invokefunction","function_id":"utf8","data":"`)
	suffix := []byte(`"}`)
	before := strings.Repeat("x", maxJSONFragmentBytes-2-len(prefix))
	afterLength := targetSize - len(prefix) - len(before) - len("😀") - len(suffix)
	message := []byte(string(prefix) + before + "😀" + strings.Repeat("x", afterLength) + string(suffix))
	if len(message) != targetSize || !json.Valid(message) || !strings.Contains(string(message), "😀") {
		t.Fatal("failed to construct valid UTF-8 envelope with a code point split across frames")
	}
	if err := New(server.url()).writeJSON(ctx, conn, message); err != nil {
		t.Fatal(err)
	}
	frames := server.readFrames(t, (targetSize+maxJSONFragmentBytes-1)/maxJSONFragmentBytes+1)
	assertFrameMessage(t, frames, message, true)
}

func TestJSONConcurrentFragmentedWritesDoNotInterleave(t *testing.T) {
	server := newRawWSServer(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	conn, _, err := websocket.Dial(ctx, server.url(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer conn.CloseNow()

	const sendCount = 3
	const messageSize = maxJSONFrameBytes + 100
	messages := make([][]byte, sendCount)
	for i := range messages {
		messages[i] = envelopeWithSize(messageSize, fmt.Sprintf("concurrent-%d", i))
	}
	start := make(chan struct{})
	writeErrors := make(chan error, sendCount)
	var writers sync.WaitGroup
	for _, message := range messages {
		writers.Add(1)
		go func(message []byte) {
			defer writers.Done()
			<-start
			writeErrors <- New(server.url()).writeJSON(ctx, conn, message)
		}(message)
	}
	close(start)
	writers.Wait()
	close(writeErrors)
	for err := range writeErrors {
		if err != nil {
			t.Fatal(err)
		}
	}

	framesPerMessage := (messageSize+maxJSONFragmentBytes-1)/maxJSONFragmentBytes + 1
	frames := server.readFrames(t, sendCount*framesPerMessage)
	seen := make(map[string]bool, sendCount)
	for i := 0; i < len(frames); i += framesPerMessage {
		group := frames[i : i+framesPerMessage]
		var wire []byte
		for _, frame := range group {
			wire = append(wire, frame.payload...)
		}
		var envelope struct {
			FunctionID string `json:"function_id"`
		}
		if err := json.Unmarshal(wire, &envelope); err != nil {
			t.Fatalf("concurrent message frames interleaved: %v", err)
		}
		if seen[envelope.FunctionID] {
			t.Fatalf("duplicate or interleaved message %q", envelope.FunctionID)
		}
		seen[envelope.FunctionID] = true
		if !strings.HasPrefix(envelope.FunctionID, "concurrent-") {
			t.Fatalf("unexpected message identity %q", envelope.FunctionID)
		}
		for j, frame := range group {
			wantOpcode := byte(0)
			if j == 0 {
				wantOpcode = 1
			}
			if frame.opcode != wantOpcode || frame.fin != (j == len(group)-1) {
				t.Fatalf("message %q frame %d is interleaved or malformed: %+v", envelope.FunctionID, j, frameSummary(group))
			}
		}
	}
	if len(seen) != sendCount {
		t.Fatalf("received %d distinct messages, want %d", len(seen), sendCount)
	}
}
