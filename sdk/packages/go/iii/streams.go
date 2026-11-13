package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"iter"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"weak"

	"github.com/google/uuid"
	"github.com/invopop/jsonschema"
)

// This file holds call-native streams: InputStream and OutputStream are values in call
// payloads and results, the Go counterpart of the streams of the Node, Python and Rust
// SDKs (sdk/packages/rust/iii/src/streams.rs is the model: Go payloads are raw JSON, as
// Rust's are serde values).
//
// A stream crosses a call as the ref of one end of an engine pipe
// ({channel_id, access_key, direction}, the JSON every SDK reads). The SDK makes the
// pipe and moves the bytes:
//
//   - What it sends (a call's payload, a handler's result) holds streams built here
//     (InputStreamFromReader, InputStreamFromBytes, OutputStreamToWriter, or a received
//     stream already read from or written to). Their MarshalJSON writes a placeholder
//     ref ("local:<token>", no access key) and keeps the stream in a process-wide
//     registry; Call and the handler's answer replace each placeholder with the ref of a
//     new pipe (created in parallel) and start a goroutine that pumps the data: the
//     source into the pipe, or the pipe into the sink. A received stream not yet used
//     marshals to its own ref (it is forwarded): no byte passes through this worker.
//   - What it receives unmarshals: a "read" ref is an InputStream, a "write" ref an
//     OutputStream, in a handler's input and in a call's result alike. The refs in a
//     handler's input are tracked while the handler runs (a typed or an untyped handler
//     alike), and the refs in a call's result for a while after it: unmarshalling one
//     gives the tracked stream, bound to the client that received it. A captured input
//     or output of an observed call arrives as a StreamInfo.
//
// In-process (a call this worker dispatches to its own function) a stream passes as is:
// the handler reads the caller's source, or writes into the caller's sink, with no
// pipe.

// placeholderPrefix is the channel_id prefix of a placeholder ref.
const placeholderPrefix = "local:"

// streamTTL is how long a placeholder nobody claimed, and a ref of a call result nobody
// unmarshalled, are kept: the engine drops an end nobody attaches after about as long.
const streamTTL = 5 * time.Minute

// walkDepth bounds the walks of payloads and results: a stream nested deeper is not
// seen. The engine parses messages with serde_json, which refuses JSON nested deeper
// than 128 levels, so no value it relays holds a stream deeper than this.
const walkDepth = 128

var streamIDs atomic.Uint64

func nextStreamID() uint64 { return streamIDs.Add(1) }

// sentReader is the reader id of a stream that was sent on: every later read fails.
const sentReader = ^uint64(0)

// sentHolder is the holder of a stream a placeholder carries to an in-process receiver.
const sentHolder = ^uint64(0)

// =====================================================================================
// Binding: which engine a ref-backed stream dials
// =====================================================================================

// liveClients are the clients of this process not yet closed: a received stream that
// is bound to none binds to the only one. Weak, so a client dropped without Close does
// not stay in memory for it.
var liveClients struct {
	mu  sync.Mutex
	set map[weak.Pointer[Client]]struct{}
}

func addLiveClient(c *Client) {
	liveClients.mu.Lock()
	defer liveClients.mu.Unlock()
	if liveClients.set == nil {
		liveClients.set = map[weak.Pointer[Client]]struct{}{}
	}
	liveClients.set[weak.Make(c)] = struct{}{}
}

func removeLiveClient(c *Client) {
	liveClients.mu.Lock()
	delete(liveClients.set, weak.Make(c))
	liveClients.mu.Unlock()
}

// onlyLiveClient is the only client of this process not yet closed, if there is exactly
// one.
func onlyLiveClient() *Client {
	liveClients.mu.Lock()
	defer liveClients.mu.Unlock()
	var only *Client
	for p := range liveClients.set {
		c := p.Value()
		if c == nil {
			delete(liveClients.set, p)
			continue
		}
		if only != nil {
			return nil
		}
		only = c
	}
	return only
}

const unboundMessage = "the stream is not bound to a client: it was received outside a handler and " +
	"outside a call result while no single Client is alive; call Bind before reading or writing it"

// resolveEndpoint is bound, or else the endpoint of the only live client, which bound is
// then set to. Called with the stream's lock held.
func resolveEndpoint(bound **endpoint) (*endpoint, error) {
	if *bound != nil {
		return *bound, nil
	}
	c := onlyLiveClient()
	if c == nil {
		return nil, errors.New("iii: " + unboundMessage)
	}
	*bound = c.streamEndpoint()
	return *bound, nil
}

// =====================================================================================
// Placeholders: streams of this process until a call or a result carries them
// =====================================================================================

type localStream struct {
	at  time.Time
	in  *inputInner
	out *outputInner
}

// placeholders are the streams marshalled as placeholders, by token. A call or a result
// that carries one claims it (a pipe replaces it); a handler in this process that
// receives one takes the stream itself.
var placeholders struct {
	mu sync.Mutex
	m  map[string]localStream
}

// placeholdersUsed is set once a placeholder was made: until then no value needs a walk.
var placeholdersUsed atomic.Bool

func newToken() string { return strings.ReplaceAll(uuid.NewString(), "-", "") }

func registerPlaceholder(token string, entry localStream) {
	now := time.Now()
	entry.at = now
	placeholders.mu.Lock()
	if placeholders.m == nil {
		placeholders.m = map[string]localStream{}
	}
	for t, e := range placeholders.m {
		if now.Sub(e.at) >= streamTTL {
			delete(placeholders.m, t)
		}
	}
	placeholders.m[token] = entry
	placeholders.mu.Unlock()
	placeholdersUsed.Store(true)
}

func claimPlaceholder(token string) (localStream, bool) {
	placeholders.mu.Lock()
	defer placeholders.mu.Unlock()
	e, ok := placeholders.m[token]
	if ok {
		delete(placeholders.m, token)
	}
	return e, ok
}

func peekPlaceholder(token string) (localStream, bool) {
	placeholders.mu.Lock()
	defer placeholders.mu.Unlock()
	e, ok := placeholders.m[token]
	return e, ok
}

func placeholderJSON(token string, dir ChannelDirection) []byte {
	out, _ := json.Marshal(StreamChannelRef{ChannelID: placeholderPrefix + token, Direction: dir})
	return out
}

func refJSON(ref StreamChannelRef) ([]byte, error) { return json.Marshal(ref) }

func gone(channelID string) error {
	return fmt.Errorf("iii: stream %s is no longer available: it was already received or sent, or not claimed within 5 minutes", channelID)
}

// =====================================================================================
// Received refs: the streams of handler inputs and call results
// =====================================================================================

type receivedKey struct {
	channelID string
	dir       ChannelDirection
}

type receivedEntry struct {
	in  *inputInner
	out *outputInner
	// guard is the invocation whose input holds it; nil for a call result.
	guard *streamGuard
	at    time.Time
}

// transferred: its ref was sent on, so a ref of it that comes back is a stream of its
// own again.
func (e *receivedEntry) transferred() bool {
	if e.in != nil {
		return e.in.forwarded()
	}
	return e.out.forwarded()
}

// received are the refs that invocations of this process received, while their
// handlers run, and the refs of call results, for streamTTL: unmarshalling one gives the
// tracked stream, and passing one on as raw JSON marks it as sent.
var received struct {
	mu sync.Mutex
	m  map[receivedKey]*receivedEntry
}

var receivedLen atomic.Int64

func lookupReceived(key receivedKey) *receivedEntry {
	if receivedLen.Load() == 0 {
		return nil
	}
	received.mu.Lock()
	defer received.mu.Unlock()
	e := received.m[key]
	if e == nil || e.transferred() {
		return nil
	}
	return e
}

// storeReceived tracks entry under key, replacing what was there. Expired entries of
// call results go.
func storeReceived(key receivedKey, entry *receivedEntry) {
	now := time.Now()
	entry.at = now
	received.mu.Lock()
	if received.m == nil {
		received.m = map[receivedKey]*receivedEntry{}
	}
	for k, e := range received.m {
		if e.guard == nil && now.Sub(e.at) >= streamTTL {
			delete(received.m, k)
		}
	}
	received.m[key] = entry
	receivedLen.Store(int64(len(received.m)))
	received.mu.Unlock()
}

// dropReceived removes the entries of guard among keys.
func dropReceived(keys []receivedKey, guard *streamGuard) {
	if len(keys) == 0 {
		return
	}
	received.mu.Lock()
	for _, k := range keys {
		if e := received.m[k]; e != nil && e.guard == guard {
			delete(received.m, k)
		}
	}
	receivedLen.Store(int64(len(received.m)))
	received.mu.Unlock()
}

// =====================================================================================
// InputStream
// =====================================================================================

type readStatus uint8

const (
	// readUnread: nothing read yet.
	readUnread readStatus = iota
	// readReading: a reader started.
	readReading
	// readEnded: the writer closed it and everything was read.
	readEnded
	// readFailed: it ended with an error (an abort, a failed dial).
	readFailed
	// readCancelled: cancelled by its reader, or by the handler guard: every read fails
	// with STREAM_CLOSED.
	readCancelled
	// readForwarded: its own ref was sent on; the other side reads it.
	readForwarded
	// readPiped: a pump took the rest of it, to send it on through a pipe.
	readPiped
)

type inputInner struct {
	// ref is the pipe end it reads, for a stream received or made with a pipe.
	ref *StreamChannelRef
	// ioReader is the reader id of the stream's own Read.
	ioReader uint64

	// readMu: one frame is read at a time.
	readMu sync.Mutex

	mu      sync.Mutex
	bound   *endpoint
	status  readStatus
	failure error
	// src is the frames still to read: a local source from the start, the socket of a
	// ref-backed stream from its first read.
	src frameSource
	// reader is the reader that owns the stream (one reader); 0 before any.
	reader uint64
	// leftover is the rest of a binary frame a Read buffer could not take.
	leftover []byte
	// holder is the invocation guard that settles it (0 for none, sentHolder while a
	// placeholder carries it to an in-process receiver).
	holder uint64
	token  string
	// movedTo is the stream an in-process call handed the rest of it to, and taken the
	// source a pump took: a failure of the call that sent it reaches them.
	movedTo *inputInner
	taken   frameSource
}

const (
	cancelledMessage      = "the stream was cancelled"
	guardCancelledMessage = "the stream was cancelled when its handler returned: read it before returning"
)

func newInput(ref *StreamChannelRef, src frameSource, ep *endpoint) *inputInner {
	return &inputInner{ref: ref, src: src, bound: ep, ioReader: nextStreamID()}
}

func errAlreadyRead() error {
	return streamInUseError("the stream already has a reader: an InputStream is read once, by one reader, and not after it was sent on")
}

func errReadToTheEnd() error {
	return streamInUseError("the stream was already read to the end: there is nothing left to send")
}

func errAlreadySent() error {
	return streamInUseError("the stream was already sent: a stream can be sent once")
}

func errSentTwice() error {
	return streamInUseError("a stream can be sent once: the same stream appears twice")
}

// next returns the next frame for reader; a second reader gets STREAM_IN_USE.
func (in *inputInner) next(ctx context.Context, reader uint64) (StreamFrame, error) {
	in.readMu.Lock()
	defer in.readMu.Unlock()
	in.mu.Lock()
	// A cancelled stream fails every read, whoever reads it: what was read is not the whole
	// stream.
	if in.status == readCancelled {
		defer in.mu.Unlock()
		return StreamFrame{}, in.failure
	}
	if in.reader == 0 {
		in.reader = reader
	} else if in.reader != reader {
		in.mu.Unlock()
		return StreamFrame{}, errAlreadyRead()
	}
	switch in.status {
	case readEnded:
		in.mu.Unlock()
		return StreamFrame{}, io.EOF
	case readForwarded, readPiped:
		in.mu.Unlock()
		return StreamFrame{}, errAlreadyRead()
	case readFailed:
		// Every read after a failure fails the same way: the stream is incomplete.
		defer in.mu.Unlock()
		return StreamFrame{}, in.failure
	}
	in.status = readReading
	if len(in.leftover) > 0 {
		rest := in.leftover
		in.leftover = nil
		in.mu.Unlock()
		return StreamFrame{Type: FrameBinary, Data: rest}, nil
	}
	if in.src == nil {
		src, err := in.openLocked()
		if err != nil {
			in.status, in.failure = readFailed, err
			in.mu.Unlock()
			return StreamFrame{}, err
		}
		in.src = src
	}
	src := in.src
	in.mu.Unlock()

	frame, err := src.next(ctx)

	in.mu.Lock()
	defer in.mu.Unlock()
	switch in.status {
	case readCancelled, readFailed:
		// Cancelled, or failed by the call that sent it, while this read was in flight.
		return StreamFrame{}, in.failure
	case readForwarded, readPiped:
		return StreamFrame{}, errAlreadyRead()
	}
	switch {
	case err == io.EOF:
		in.status, in.src = readEnded, nil
		return StreamFrame{}, io.EOF
	case err != nil:
		in.status, in.failure, in.src = readFailed, err, nil
		go src.cancel()
		return StreamFrame{}, err
	}
	return frame, nil
}

// openLocked opens the socket of a ref-backed stream.
func (in *inputInner) openLocked() (frameSource, error) {
	if in.ref == nil {
		return emptySource{}, nil
	}
	ep, err := resolveEndpoint(&in.bound)
	if err != nil {
		return nil, err
	}
	return &wsSource{ep: ep, ref: *in.ref}, nil
}

// putBack keeps the rest of a binary frame for the next read.
func (in *inputInner) putBack(rest []byte) {
	in.mu.Lock()
	if in.status == readReading {
		in.leftover = rest
	}
	in.mu.Unlock()
}

// cancel stops reading: the socket, if open, closes (the engine then closes the
// writer), a local source is released, and a read in flight and every later read fail
// with STREAM_CLOSED. A stream never read opens no socket.
func (in *inputInner) cancel() { in.end(readCancelled, streamClosedError(cancelledMessage)) }

// end stops the stream with status (readCancelled or readFailed) and err: a read in
// flight and every later read fail with err, never io.EOF, since what was read is not the
// whole stream; the source is released. A stream that ended, failed or was sent on is
// left as it is, except that a failure reaches what took the rest of it in-process (a
// receiver, or a pump's source).
func (in *inputInner) end(status readStatus, err error) {
	in.mu.Lock()
	switch in.status {
	case readUnread, readReading:
	case readPiped:
		next, taken := in.movedTo, in.taken
		in.mu.Unlock()
		switch {
		case status != readFailed:
		case next != nil:
			next.end(status, err)
		case taken != nil:
			taken.cancel()
		}
		return
	default:
		in.mu.Unlock()
		return
	}
	in.status, in.failure, in.leftover = status, err, nil
	src := in.src
	in.src = nil
	in.mu.Unlock()
	if src != nil {
		src.cancel()
	}
}

// handOver gives a stream sent in-process to its receiver: its first reader reads the
// rest.
func (in *inputInner) handOver(holder uint64) {
	in.mu.Lock()
	if in.status == readReading {
		in.reader = 0
	}
	if holder != 0 {
		in.holder = holder
	}
	in.mu.Unlock()
}

// settle is the guard's rule for input (R3): a stream the handler of guard id started
// reading and did not finish is cancelled. One it sent on is the receiver's to settle.
func (in *inputInner) settle(id uint64) {
	in.mu.Lock()
	reading := in.status == readReading && in.holder == id && in.reader != 0
	in.mu.Unlock()
	if reading {
		in.end(readCancelled, streamClosedError(guardCancelledMessage))
	}
}

func (in *inputInner) forwarded() bool {
	in.mu.Lock()
	defer in.mu.Unlock()
	return in.status == readForwarded
}

func (in *inputInner) readToTheEnd() bool {
	in.mu.Lock()
	defer in.mu.Unlock()
	return in.status == readEnded
}

// sentOn: the stream was sent (its ref forwarded, or the rest of it taken by a pump or an
// in-process receiver); it is not sent again.
func (in *inputInner) sentOn() bool {
	in.mu.Lock()
	defer in.mu.Unlock()
	return in.status == readForwarded || in.status == readPiped
}

// transfer marks a received ref passed on as raw JSON: the guard leaves it alone.
func (in *inputInner) transfer() {
	in.mu.Lock()
	if in.status == readUnread {
		in.status, in.reader = readForwarded, sentReader
	}
	in.mu.Unlock()
}

// marshal is what the stream marshals to: its own ref when it was received and not yet
// read (forwarded), else a placeholder the outbound walk pumps, refuses, or hands to an
// in-process receiver.
func (in *inputInner) marshal() ([]byte, error) {
	in.mu.Lock()
	if in.ref != nil && in.status == readUnread {
		in.status, in.reader = readForwarded, sentReader
		ref := *in.ref
		in.mu.Unlock()
		return refJSON(ref)
	}
	in.holder = sentHolder
	if in.token == "" {
		in.token = newToken()
	}
	token := in.token
	in.mu.Unlock()
	registerPlaceholder(token, localStream{in: in})
	return placeholderJSON(token, ChannelRead), nil
}

// takeSource takes the rest of the stream, for a pump that sends it on (R5).
func (in *inputInner) takeSource() (frameSource, error) {
	if !in.readMu.TryLock() {
		return nil, streamInUseError("the stream is being read: it cannot be sent on while a read is in flight")
	}
	defer in.readMu.Unlock()
	in.mu.Lock()
	defer in.mu.Unlock()
	switch in.status {
	case readEnded:
		return nil, errReadToTheEnd()
	case readFailed:
		return nil, in.failure
	case readCancelled:
		return nil, streamClosedError("the stream was cancelled: there is nothing left to send")
	case readForwarded, readPiped:
		return nil, errAlreadySent()
	}
	src := in.src
	if src == nil {
		opened, err := in.openLocked()
		if err != nil {
			return nil, err
		}
		src = opened
	}
	if len(in.leftover) > 0 {
		src = &prependSource{first: &StreamFrame{Type: FrameBinary, Data: in.leftover}, rest: src}
	}
	in.src, in.leftover, in.taken = nil, nil, src
	in.status, in.reader = readPiped, sentReader
	return src, nil
}

// moveOut hands the rest of a stream an in-process call sends to its receiver: a new
// stream that reads it, under the same placeholder. This one counts as sent, as one a
// pipe carries does: it is not read or sent again (STREAM_IN_USE).
func (in *inputInner) moveOut() (*inputInner, error) {
	src, err := in.takeSource()
	if err != nil {
		return nil, err
	}
	in.mu.Lock()
	defer in.mu.Unlock()
	recv := &inputInner{src: src, bound: in.bound, ioReader: nextStreamID(), holder: sentHolder, token: in.token}
	in.movedTo, in.taken = recv, nil
	return recv, nil
}

// InputStream is the read side of a stream: the bytes (and messages) a function
// receives.
//
// An InputStream arrives as a field of a handler's input or of a call's result (a
// "read" ref unmarshals to it), or is built to be sent: [InputStreamFromReader],
// [InputStreamFromBytes], or the input end of [Client.Pipe]. Put it in a call payload
// or a handler result (as a *InputStream field, or in a map) and the SDK streams it to
// the other side.
//
// Read it once, one way: as an [io.Reader] (Read), with [InputStream.ReadAll],
// [InputStream.Text], [InputStream.Lines], [InputStream.Messages] (text frames) or
// [InputStream.Frames] (both kinds, in order). Byte reads skip messages. A second
// reader gets STREAM_IN_USE ([CodeStreamInUse]): Read is one reader across calls, and
// each of the other methods is a reader of its own. The socket opens on the first read.
// A stream read to the end returns io.EOF on further reads; one cancelled before its end
// ([InputStream.Cancel], [InputStream.Close], a break out of Lines, Messages or Frames,
// or the guard below) fails every later read, and a read in flight, with STREAM_CLOSED
// ([CodeStreamClosed]), never io.EOF.
//
// An abort (its writer failed or called [OutputStream.Abort]) ends a read with an
// [*InvocationError] of code [CodeStreamAborted], after the data sent before it.
//
// In a handler, a stream the handler started reading and did not finish is cancelled
// when the handler returns (a goroutine still reading it gets STREAM_CLOSED: read it
// before returning); one never read is left alone. One sent on unread (in a call
// payload or the result) is forwarded: its ref goes, and no byte passes through this
// worker. One sent on after reading is pumped from where it is. A stream is sent once:
// sending it twice, or after it was read to the end, fails with STREAM_IN_USE.
//
// A received stream binds to the client that received it (in a handler's input or a
// call's result); one unmarshalled from JSON that no client received binds to the only
// [Client] not yet closed, or call [InputStream.Bind]. An InputStream is a handle:
// copies of it are the same stream. Its methods are safe for concurrent use, but reads
// are not meant to be shared.
type InputStream struct {
	inner *inputInner
}

// InputStreamFromReader returns a stream of what r yields, read in chunks of up to
// 64 KiB when the stream is read or sent. A read error aborts the stream with its
// message: the reader gets STREAM_ABORTED, not a short stream that looks complete. When
// r is an [io.Closer], the stream closes it once it ends, fails or is cancelled (it
// owns r, as an http.Request owns its Body).
//
//	f, err := os.Open("report.csv")
//	if err != nil { ... }
//	payload, _ := json.Marshal(map[string]any{"filename": "report.csv", "body": iii.InputStreamFromReader(f)})
//	result, err := worker.Call(ctx, iii.CallRequest{FunctionID: "files::process", Payload: payload})
func InputStreamFromReader(r io.Reader) *InputStream {
	if r == nil {
		return &InputStream{inner: newInput(nil, emptySource{}, nil)}
	}
	return &InputStream{inner: newInput(nil, &readerSource{r: r}, nil)}
}

// InputStreamFromBytes returns a stream of data. The stream keeps data: do not change
// it until the stream was read or sent.
func InputStreamFromBytes(data []byte) *InputStream {
	return &InputStream{inner: newInput(nil, &bytesSource{rest: data}, nil)}
}

func (s *InputStream) get() (*inputInner, error) {
	if s == nil || s.inner == nil {
		return nil, errors.New("iii: the InputStream is empty: build one with InputStreamFromReader or InputStreamFromBytes, or receive one")
	}
	return s.inner, nil
}

// Bind binds a received stream to c: its socket opens on c's engine, with c's handshake
// headers. Needed only for a stream unmarshalled from JSON that no client received,
// while several clients are alive. It returns s.
func (s *InputStream) Bind(c *Client) *InputStream {
	if in, err := s.get(); err == nil && c != nil {
		in.mu.Lock()
		in.bound = c.streamEndpoint()
		in.mu.Unlock()
	}
	return s
}

// Read reads the stream's bytes (its binary frames) and implements [io.Reader]. It
// returns io.EOF once the writer closed the stream and everything was read. An abort is
// an error, never io.EOF: the bytes read so far are incomplete. Read is one reader:
// calls of Read continue each other, and another read method fails after it.
func (s *InputStream) Read(p []byte) (int, error) {
	in, err := s.get()
	if err != nil {
		return 0, err
	}
	if len(p) == 0 {
		return 0, nil
	}
	for {
		frame, err := in.next(context.Background(), in.ioReader)
		if err != nil {
			return 0, err
		}
		if frame.Type != FrameBinary || len(frame.Data) == 0 {
			continue
		}
		n := copy(p, frame.Data)
		if n < len(frame.Data) {
			in.putBack(frame.Data[n:])
		}
		return n, nil
	}
}

// Close cancels the stream (see [InputStream.Cancel]) and returns nil, so an
// InputStream is an [io.ReadCloser]. After a Close before the end, reads fail with
// STREAM_CLOSED.
func (s *InputStream) Close() error {
	s.Cancel()
	return nil
}

// frames yields every frame for one new reader; a break cancels the stream.
func (s *InputStream) frames(ctx context.Context) iter.Seq2[StreamFrame, error] {
	return func(yield func(StreamFrame, error) bool) {
		in, err := s.get()
		if err != nil {
			yield(StreamFrame{}, err)
			return
		}
		reader := nextStreamID()
		for {
			frame, err := in.next(ctx, reader)
			if err == io.EOF {
				return
			}
			if err != nil {
				yield(StreamFrame{}, err)
				return
			}
			if !yield(frame, nil) {
				in.cancel()
				return
			}
		}
	}
}

// ReadAll reads the whole stream (its binary frames) and returns its bytes. On an
// error, an abort included, it returns the bytes read so far with the error. ctx bounds
// the reads: when it ends, the stream is cancelled.
func (s *InputStream) ReadAll(ctx context.Context) ([]byte, error) {
	var out []byte
	for frame, err := range s.frames(ctx) {
		if err != nil {
			return out, err
		}
		if frame.Type == FrameBinary {
			out = append(out, frame.Data...)
		}
	}
	return out, nil
}

// Text reads the whole stream as text (its bytes, as a string).
func (s *InputStream) Text(ctx context.Context) (string, error) {
	data, err := s.ReadAll(ctx)
	return string(data), err
}

// Lines yields the stream's lines, without their "\n" (or "\r\n"); the last one may have
// no newline. After an error it yields ("", err) and stops. Breaking out of the loop
// cancels the stream.
//
//	for line, err := range csv.Lines(ctx) {
//	    if err != nil { return err }
//	    render(line)
//	}
func (s *InputStream) Lines(ctx context.Context) iter.Seq2[string, error] {
	return func(yield func(string, error) bool) {
		var buf []byte
		for frame, err := range s.frames(ctx) {
			if err != nil {
				yield("", err)
				return
			}
			if frame.Type != FrameBinary {
				continue
			}
			buf = append(buf, frame.Data...)
			for {
				i := bytes.IndexByte(buf, '\n')
				if i < 0 {
					break
				}
				line := buf[:i]
				buf = buf[i+1:]
				if !yield(string(bytes.TrimSuffix(line, []byte("\r"))), nil) {
					return
				}
			}
		}
		if len(buf) > 0 {
			yield(string(buf), nil)
		}
	}
}

// Messages yields the stream's messages (its text frames), each as JSON. A text frame
// that is not JSON yields its text as a JSON string. Bytes are skipped. After an error
// it yields (nil, err) and stops. Breaking out of the loop cancels the stream.
//
//	for msg, err := range tokens.Messages(ctx) {
//	    if err != nil { return err }
//	    var m struct{ Token string `json:"token"` }
//	    _ = json.Unmarshal(msg, &m)
//	}
func (s *InputStream) Messages(ctx context.Context) iter.Seq2[json.RawMessage, error] {
	return func(yield func(json.RawMessage, error) bool) {
		for frame, err := range s.frames(ctx) {
			if err != nil {
				yield(nil, err)
				return
			}
			if frame.Type != FrameText {
				continue
			}
			if !yield(decodeMessage(frame.Data), nil) {
				return
			}
		}
	}
}

// decodeMessage is a text frame as a message: the frame when it is JSON, else its text
// as a JSON string.
func decodeMessage(text []byte) json.RawMessage {
	if json.Valid(text) {
		return json.RawMessage(text)
	}
	quoted, _ := json.Marshal(string(text))
	return quoted
}

// Frames yields every frame of the stream in order: bytes ([FrameBinary]) and messages
// ([FrameText]). After an error it yields it and stops. Breaking out of the loop
// cancels the stream.
func (s *InputStream) Frames(ctx context.Context) iter.Seq2[StreamFrame, error] {
	return s.frames(ctx)
}

// Cancel stops reading: the socket, if open, closes, and the engine then closes the
// writer, as it does when a reader leaves. A read in flight and every later read fail
// with STREAM_CLOSED ([CodeStreamClosed]): what was read is not the whole stream. A
// stream never read opens no socket; a local source is released (an io.Closer is
// closed). Cancel does nothing to a stream read to the end, which keeps returning
// io.EOF.
func (s *InputStream) Cancel() {
	if in, err := s.get(); err == nil {
		in.cancel()
	}
}

// MarshalJSON writes the stream as the ref that takes its place in a payload or a
// result: its own ref when it was received and not yet read (it is forwarded), else a
// placeholder the SDK replaces with the ref of a new pipe when the call or the answer is
// sent. Marshal a stream only to send it: a marshalled stream counts as sent.
func (s InputStream) MarshalJSON() ([]byte, error) {
	if s.inner == nil {
		return []byte("null"), nil
	}
	return s.inner.marshal()
}

// UnmarshalJSON reads a "read" ref as the stream it stands for: the one the invocation
// or the call result that received it tracks, the stream itself for a placeholder of
// this process, else a new stream of that ref. A "write" ref is an error: it is an
// [OutputStream].
func (s *InputStream) UnmarshalJSON(data []byte) error {
	if bytes.Equal(bytes.TrimSpace(data), []byte("null")) {
		return nil
	}
	ref, err := decodeStreamRef(data)
	if err != nil {
		return err
	}
	if ref.Direction != ChannelRead {
		return errors.New(`iii: expected a stream to read (direction "read"): a "write" ref is an OutputStream`)
	}
	if e := lookupReceived(receivedKey{ref.ChannelID, ChannelRead}); e != nil && e.in != nil {
		// In-process, the receiver's guard tracks the caller's stream: it is the
		// receiver's now.
		if token, ok := placeholderToken(ref); ok {
			claimPlaceholder(token)
		}
		s.inner = e.in
		return nil
	}
	if token, ok := placeholderToken(ref); ok {
		e, ok := claimPlaceholder(token)
		if !ok || e.in == nil {
			if ok {
				registerPlaceholder(token, e)
			}
			return gone(ref.ChannelID)
		}
		e.in.handOver(0)
		s.inner = e.in
		return nil
	}
	s.inner = newInput(&ref, nil, nil)
	return nil
}

// JSONSchema is the schema of a stream to read, for [RegisterFunctionTyped] and
// [InferSchema]: the ref object, marked with "x-iii-stream": "read".
func (InputStream) JSONSchema() *jsonschema.Schema {
	return streamSchema("InputStream", ChannelRead,
		`A stream the function reads: the ref of the read end of an engine pipe ({channel_id, access_key, direction: "read"}). The SDK opens it on first read.`)
}

func streamSchema(title string, dir ChannelDirection, description string) *jsonschema.Schema {
	props := jsonschema.NewProperties()
	props.Set("channel_id", &jsonschema.Schema{Type: "string"})
	props.Set("access_key", &jsonschema.Schema{Type: "string"})
	props.Set("direction", &jsonschema.Schema{Type: "string", Const: string(dir)})
	return &jsonschema.Schema{
		Title:       title,
		Description: description,
		Type:        "object",
		Properties:  props,
		Required:    []string{"channel_id", "access_key", "direction"},
		Extras:      map[string]any{"x-iii-stream": string(dir)},
	}
}

// decodeStreamRef reads the ref fields of a stream value; extra keys are tolerated.
func decodeStreamRef(data []byte) (StreamChannelRef, error) {
	var wire struct {
		ChannelID *string `json:"channel_id"`
		AccessKey *string `json:"access_key"`
		Direction string  `json:"direction"`
	}
	if err := json.Unmarshal(data, &wire); err != nil {
		return StreamChannelRef{}, fmt.Errorf("iii: a stream is a ref object {channel_id, access_key, direction}: %w", err)
	}
	if wire.ChannelID == nil || wire.AccessKey == nil || (wire.Direction != "read" && wire.Direction != "write") {
		return StreamChannelRef{}, errors.New(`iii: a stream is a ref object {channel_id, access_key, direction: "read" or "write"}`)
	}
	return StreamChannelRef{ChannelID: *wire.ChannelID, AccessKey: *wire.AccessKey, Direction: ChannelDirection(wire.Direction)}, nil
}

func placeholderToken(ref StreamChannelRef) (string, bool) {
	if ref.AccessKey != "" {
		return "", false
	}
	return strings.CutPrefix(ref.ChannelID, placeholderPrefix)
}

// =====================================================================================
// OutputStream
// =====================================================================================

type useState uint8

const (
	// useUnused: not written, sent to, or bridged yet.
	useUnused useState = iota
	// useUsed: written, sent to, or bridged.
	useUsed
	// useForwarded: its own ref was sent on; the other side writes it.
	useForwarded
	// usePiped: a pump writes into it what another function writes into a pipe.
	usePiped
)

type outputInner struct {
	// ref is the pipe end it writes, for a stream received or made with a pipe.
	ref *StreamChannelRef

	// ioMu serializes writes, sends, close and the abort of a pipe end.
	ioMu sync.Mutex
	ws   *wsWriter
	// sink is the local writer of a stream made with OutputStreamToWriter; written
	// until the stream ends.
	sink io.Writer

	mu      sync.Mutex
	bound   *endpoint
	usage   useState
	ended   bool
	aborted bool
	reason  string
	// closing: a close is in progress; an abort then does nothing.
	closing bool
	// sinkErr is the error its local sink failed with (a Write, or the Flush or Close
	// that ends it): the call that passed it fails with it (failedSink).
	sinkErr error
	// ioCancel ends the write or send of a pipe end in flight: an abort does not wait
	// behind it.
	ioCancel context.CancelFunc
	// done is closed once the stream closed or aborted.
	done     chan struct{}
	keptOpen atomic.Bool
	token    string
}

func newOutput(ref *StreamChannelRef, sink io.Writer, ep *endpoint) *outputInner {
	return &outputInner{ref: ref, sink: sink, bound: ep, done: make(chan struct{})}
}

func errStreamEnded() error {
	return streamClosedError("the stream is closed: it was closed or aborted")
}

// sinkFailedReason is the reason a stream whose local sink failed with err aborts with.
func sinkFailedReason(err error) string { return "the sink failed: " + err.Error() }

// sinkFailure is what a call fails with when a sink it passed failed with err:
// STREAM_ABORTED, wrapping err.
func sinkFailure(err error) error {
	return &InvocationError{Code: CodeStreamAborted, Message: sinkFailedReason(err), cause: err}
}

// closedLocked is what a write gets once the stream ended: STREAM_CLOSED, wrapping the
// sink's error when its sink failed. Called with mu held.
func (o *outputInner) closedLocked() error {
	if o.sinkErr != nil {
		return &InvocationError{Code: CodeStreamClosed, Message: "the stream is closed: " + sinkFailedReason(o.sinkErr), cause: o.sinkErr}
	}
	return errStreamEnded()
}

// endedError is closedLocked once the stream ended, else nil. A write or a close checks
// it before taking ioMu, so one after an abort does not wait behind a write still
// blocked on the sink.
func (o *outputInner) endedError() error {
	o.mu.Lock()
	defer o.mu.Unlock()
	if !o.ended {
		return nil
	}
	return o.closedLocked()
}

// failSink: the local sink failed with err. The stream aborts, so nothing more is
// written to it (the pump that fills it stops, and the handler's next write fails with
// STREAM_CLOSED); the sink is aborted (CloseWithError, when it has it); and the call
// that passed it fails with sinkFailure(err). It returns what the write that failed
// gets. A stream that had ended (aborted while the write was in flight) keeps its end.
func (o *outputInner) failSink(err error) error {
	o.mu.Lock()
	if o.ended {
		defer o.mu.Unlock()
		return o.closedLocked()
	}
	reason := sinkFailedReason(err)
	o.sinkErr = err
	o.ended, o.aborted, o.reason = true, true, reason
	closed := o.closedLocked()
	o.mu.Unlock()
	close(o.done)
	abortSink(o.sink, reason)
	return closed
}

// failedSink is the failure of the first of sinks whose sink failed, nil when none did:
// what the call that passed them fails with, whatever the function returned (its writes
// failed once the sink did).
func failedSink(sinks []*outputInner) error {
	for _, o := range sinks {
		o.mu.Lock()
		err := o.sinkErr
		o.mu.Unlock()
		if err != nil {
			return sinkFailure(err)
		}
	}
	return nil
}

// begin marks a write: it fails on a stream that ended or was sent on.
func (o *outputInner) begin() error {
	o.mu.Lock()
	defer o.mu.Unlock()
	if o.ended {
		return o.closedLocked()
	}
	switch o.usage {
	case useForwarded:
		return streamInUseError("the stream was sent to another function, which writes it")
	case useUnused:
		o.usage = useUsed
	}
	return nil
}

// writerLocked is the write end of a ref-backed stream; called with ioMu held.
func (o *outputInner) writerLocked() (*wsWriter, error) {
	if o.ws == nil {
		o.mu.Lock()
		ep, err := resolveEndpoint(&o.bound)
		o.mu.Unlock()
		if err != nil {
			return nil, err
		}
		o.ws = &wsWriter{ep: ep, ref: *o.ref}
	}
	return o.ws, nil
}

// ioContext is ctx for a write or a send of a pipe end, ended by an abort too: an
// abort cancels it, so a write blocked by backpressure returns at once. Call stop once
// the write returned.
func (o *outputInner) ioContext(ctx context.Context) (_ context.Context, stop func()) {
	ctx, cancel := context.WithCancel(ctx)
	o.mu.Lock()
	if o.ended {
		cancel() // aborted since begin
	} else {
		o.ioCancel = cancel
	}
	o.mu.Unlock()
	return ctx, func() {
		o.mu.Lock()
		o.ioCancel = nil
		o.mu.Unlock()
		cancel()
	}
}

// ioError is what a write to a pipe end that failed returns: STREAM_CLOSED when the
// stream ended (an abort) while it was in flight, else err.
func (o *outputInner) ioError(err error) error {
	if err == nil {
		return nil
	}
	if ended := o.endedError(); ended != nil {
		return ended
	}
	return err
}

// isEnded: the stream closed or aborted. A close checks it before taking ioMu.
func (o *outputInner) isEnded() bool {
	o.mu.Lock()
	defer o.mu.Unlock()
	return o.ended
}

func (o *outputInner) write(ctx context.Context, data []byte) error {
	if err := o.endedError(); err != nil {
		return err
	}
	o.ioMu.Lock()
	defer o.ioMu.Unlock()
	if err := o.begin(); err != nil {
		return err
	}
	if o.ref != nil {
		w, err := o.writerLocked()
		if err != nil {
			return err
		}
		ctx, stop := o.ioContext(ctx)
		defer stop()
		return o.ioError(w.write(ctx, data))
	}
	if len(data) == 0 {
		return nil
	}
	if _, err := o.sink.Write(data); err != nil {
		return o.failSink(err)
	}
	return nil
}

func (o *outputInner) sendText(ctx context.Context, text []byte) error {
	if err := o.endedError(); err != nil {
		return err
	}
	o.ioMu.Lock()
	defer o.ioMu.Unlock()
	if err := o.begin(); err != nil {
		return err
	}
	if o.ref == nil {
		// A byte sink takes bytes only: messages are not written to it.
		return nil
	}
	w, err := o.writerLocked()
	if err != nil {
		return err
	}
	ctx, stop := o.ioContext(ctx)
	defer stop()
	return o.ioError(w.sendText(ctx, text))
}

// finish records how the stream ended; false when it had ended already.
func (o *outputInner) finish(aborted bool, reason string) bool {
	o.mu.Lock()
	if o.ended {
		o.mu.Unlock()
		return false
	}
	o.ended, o.aborted, o.reason = true, aborted, reason
	o.mu.Unlock()
	close(o.done)
	return true
}

func (o *outputInner) close() error {
	if o.isEnded() {
		return nil
	}
	o.ioMu.Lock()
	defer o.ioMu.Unlock()
	o.mu.Lock()
	if o.ended || o.usage == useForwarded {
		o.mu.Unlock()
		return nil
	}
	o.closing = true
	o.mu.Unlock()
	var err error
	if o.ref != nil {
		var w *wsWriter
		if w, err = o.writerLocked(); err == nil {
			err = w.close()
		}
	} else if err = closeSink(o.sink); err != nil {
		// Not closed: the sink may not hold the whole stream.
		_ = o.failSink(err)
		return sinkFailure(err)
	}
	o.finish(false, "")
	return err
}

func (o *outputInner) abort(reason string) error {
	if reason == "" {
		reason = "aborted"
	}
	if o.ref == nil {
		o.abortSink(reason)
		return nil
	}
	if !o.markAborted(reason) {
		return nil
	}
	return o.abortSocket(reason)
}

// abortNow aborts the stream without waiting on its socket: it has ended when abortNow
// returns (a write to it fails), and a pipe end gets its close frame in the background.
func (o *outputInner) abortNow(reason string) {
	if o.ref == nil {
		o.abortSink(reason)
		return
	}
	if o.markAborted(reason) {
		go func() { _ = o.abortSocket(reason) }()
	}
}

// abortSink aborts a local sink. It does not wait behind a write blocked on the sink
// (an io.Pipe nobody reads): CloseWithError ends that write, and a Write that ignores
// it keeps its goroutine until it returns.
func (o *outputInner) abortSink(reason string) {
	if !o.forwarded() && o.finish(true, reason) {
		abortSink(o.sink, reason)
	}
}

// markAborted ends a pipe end as aborted, at once: the write or send in flight is
// cancelled (it returns STREAM_CLOSED), and every later one fails. False when the stream
// had ended, is being closed, or was sent on.
func (o *outputInner) markAborted(reason string) bool {
	o.mu.Lock()
	if o.ended || o.closing || o.usage == useForwarded {
		o.mu.Unlock()
		return false
	}
	o.ended, o.aborted, o.reason = true, true, reason
	cancel := o.ioCancel
	o.mu.Unlock()
	close(o.done)
	if cancel != nil {
		cancel()
	}
	return true
}

// abortSocket tells the reader of an aborted pipe end: a close frame with code 1011
// (a socket never dialled is dialled first). A write cancelled mid-frame dropped the
// socket already, which the engine reads as an abort too.
func (o *outputInner) abortSocket(reason string) error {
	o.ioMu.Lock()
	defer o.ioMu.Unlock()
	w, err := o.writerLocked()
	if err != nil {
		return err
	}
	return w.abort(reason)
}

// closeSink ends a local sink cleanly: a Flush() error is flushed, an io.Closer closed.
func closeSink(sink io.Writer) error {
	if f, ok := sink.(interface{ Flush() error }); ok {
		if err := f.Flush(); err != nil {
			return err
		}
	}
	if c, ok := sink.(io.Closer); ok {
		return c.Close()
	}
	return nil
}

// abortSink ends a local sink with an error: one with CloseWithError (an
// *io.PipeWriter) gets it; any other is left as is, so a truncated stream is never
// finished as if whole.
func abortSink(sink io.Writer, reason string) {
	if c, ok := sink.(interface{ CloseWithError(error) error }); ok {
		_ = c.CloseWithError(streamAbortedError(reason))
	}
}

// outcome is how the stream ended, once it did: (ended, aborted, reason).
func (o *outputInner) outcome() (bool, bool, string) {
	o.mu.Lock()
	defer o.mu.Unlock()
	return o.ended, o.aborted, o.reason
}

func (o *outputInner) forwarded() bool {
	o.mu.Lock()
	defer o.mu.Unlock()
	return o.usage == useForwarded
}

// transfer marks a received ref passed on as raw JSON: the other side writes it now.
func (o *outputInner) transfer() {
	o.mu.Lock()
	if o.usage == useUnused && !o.ended {
		o.usage = useForwarded
	}
	o.mu.Unlock()
}

func (o *outputInner) marshal() ([]byte, error) {
	o.mu.Lock()
	if o.ref != nil && o.usage == useUnused && !o.ended {
		o.usage = useForwarded
		ref := *o.ref
		o.mu.Unlock()
		return refJSON(ref)
	}
	if o.token == "" {
		o.token = newToken()
	}
	token := o.token
	o.mu.Unlock()
	registerPlaceholder(token, localStream{out: o})
	return placeholderJSON(token, ChannelWrite), nil
}

// takeSink takes the stream as the sink of a pump.
func (o *outputInner) takeSink() error {
	o.mu.Lock()
	defer o.mu.Unlock()
	if o.ended {
		return errStreamEnded()
	}
	if o.usage == useForwarded {
		return errAlreadySent()
	}
	o.usage = usePiped
	return nil
}

// settle is the guard's rule for output (R2) once the handler returned: a stream it used
// is closed on success and aborted on failure; one it never used is closed on success
// in a plain call, and in a call through an observer only when the result is null; one
// it never used on failure, kept open, sent on or piped is left alone. It returns what
// to do, nil for nothing.
func (o *outputInner) settle(failed bool, reason string, resultNull, activation bool) func() {
	if o.keptOpen.Load() {
		return nil
	}
	o.mu.Lock()
	ended, usage := o.ended, o.usage
	o.mu.Unlock()
	if ended {
		return nil
	}
	switch {
	case usage == useUsed && !failed:
	case usage == useUsed && failed:
		return func() { _ = o.abort(reason) }
	// A plain call's untouched stream ends, so the caller's sink does.
	case usage == useUnused && !failed && !activation:
	// Through an observer the event source decides: for the HTTP worker an untouched
	// response with a value is a buffered response, and closing it first would win its
	// race.
	case usage == useUnused && !failed && resultNull:
	default:
		return nil
	}
	return func() { _ = o.close() }
}

// OutputStream is the write side of a stream: where a function writes bytes and
// messages.
//
// An OutputStream arrives as a field of a handler's input or of a call's result (a
// "write" ref unmarshals to it), or is built around a local sink with
// [OutputStreamToWriter]: put it in a call payload and the function writes into the
// sink. The call returns once the sink got the whole stream (and fails with
// STREAM_ABORTED if the writer aborted it), within the call's timeout. A call whose
// result holds a stream to write (the function returned a write end) returns without
// waiting: the caller writes it.
//
// [OutputStream.Write] sends bytes (in binary frames of up to 64 KiB) and
// [OutputStream.Send] a message (a JSON text frame); each returns once the frame was
// taken, so a loop of writes follows the reader's pace. End the stream with
// [OutputStream.Close], or fail it with [OutputStream.Abort]: the reader then gets an
// error, not a short stream. A write after either fails with STREAM_CLOSED
// ([CodeStreamClosed]). [OutputStream.Writer] adapts it to an [io.Writer]. A byte sink
// made with OutputStreamToWriter takes the bytes only: messages are dropped.
//
// In a handler, the stream is ended when the handler returns: one it used (wrote, sent
// on, or took a Writer of) is closed when the handler succeeded and aborted when it
// failed. One it never used is closed when it succeeded and left as is when it failed;
// in a call through an observer (an HTTP route, say) it is closed only when the handler
// returned nil (JSON null) and left as is with any other result, as the HTTP worker
// reads an untouched response with a value as a buffered response. Call
// [OutputStream.KeepOpen] to keep writing after the handler returned, and then close or
// abort it yourself.
//
// A received stream binds to the client that received it; see [InputStream] for the
// rules and [OutputStream.Bind]. An OutputStream is a handle: copies of it are the same
// stream. Its methods are safe for concurrent use.
type OutputStream struct {
	inner *outputInner
}

// OutputStreamToWriter returns a stream whose bytes go to w. Closing the stream flushes
// w (when it has a Flush() error method) and closes it (when it is an [io.Closer]); an
// abort closes w with the error when it has CloseWithError (an [*io.PipeWriter]) and
// otherwise leaves it as is, so a truncated stream is never finished as if whole: close
// it yourself, and remove a partly written file.
//
// When w fails (its Write, Flush or Close returns an error), the stream aborts: nothing
// more is written to w, the function's next write fails with STREAM_CLOSED, and the
// call that passed the stream fails with STREAM_ABORTED, "the sink failed: <w's
// error>", wrapping w's error (errors.Is and errors.As reach it), whatever the function
// returned.
//
// A call does not wait behind a w.Write that blocks: at the call's timeout, or when its
// ctx ends, the call returns and the stream is aborted. A Write that never returns (and
// ignores CloseWithError) keeps the goroutine that called it until it returns.
//
//	f, err := os.Create("r-42.pdf")
//	if err != nil { ... }
//	payload, _ := json.Marshal(map[string]any{"report_id": "r-42", "out": iii.OutputStreamToWriter(f)})
//	// Returns once the file holds the whole PDF.
//	_, err = worker.Call(ctx, iii.CallRequest{FunctionID: "reports::render-pdf", Payload: payload})
func OutputStreamToWriter(w io.Writer) *OutputStream {
	if w == nil {
		w = io.Discard
	}
	return &OutputStream{inner: newOutput(nil, w, nil)}
}

func (s *OutputStream) get() (*outputInner, error) {
	if s == nil || s.inner == nil {
		return nil, errors.New("iii: the OutputStream is empty: build one with OutputStreamToWriter, or receive one")
	}
	return s.inner, nil
}

// Bind binds a received stream to c: its socket opens on c's engine, with c's handshake
// headers. Needed only for a stream unmarshalled from JSON that no client received,
// while several clients are alive. It returns s.
func (s *OutputStream) Bind(c *Client) *OutputStream {
	if o, err := s.get(); err == nil && c != nil {
		o.mu.Lock()
		o.bound = c.streamEndpoint()
		o.mu.Unlock()
	}
	return s
}

// Write writes data, in binary frames of up to 64 KiB, and returns once the socket took
// them. ctx bounds the write: when it ends mid-write, the socket closes without a close
// frame and the reader gets an abort.
func (s *OutputStream) Write(ctx context.Context, data []byte) error {
	o, err := s.get()
	if err != nil {
		return err
	}
	return o.write(ctx, data)
}

// Send sends message as a JSON text frame. A json.RawMessage is sent as it is.
func (s *OutputStream) Send(ctx context.Context, message any) error {
	o, err := s.get()
	if err != nil {
		return err
	}
	text, err := json.Marshal(message)
	if err != nil {
		return err
	}
	return o.sendText(ctx, text)
}

// Close ends the stream: the reader reads to the end. A stream never written is opened
// and closed, so its reader sees an empty stream. Closing a stream already ended, or
// sent on, does nothing. It implements [io.Closer].
func (s *OutputStream) Close() error {
	o, err := s.get()
	if err != nil {
		return err
	}
	return o.close()
}

// Abort fails the stream with reason: the reader's read ends with STREAM_ABORTED
// (through an engine pipe the reason the reader sees is the engine's). A stream never
// written is opened first, so its reader always learns of it. Aborting a stream already
// ended, or being closed, does nothing. Abort does not wait behind a Write or a Send in
// flight (one blocked by a reader that does not read): that write returns STREAM_CLOSED,
// and the stream ends at once.
func (s *OutputStream) Abort(reason string) error {
	o, err := s.get()
	if err != nil {
		return err
	}
	return o.abort(reason)
}

// KeepOpen keeps the stream open after the handler that received it returns: it is
// then this code's to close or abort. It returns s.
func (s *OutputStream) KeepOpen() *OutputStream {
	if o, err := s.get(); err == nil {
		o.keptOpen.Store(true)
	}
	return s
}

// Writer returns an [io.Writer] that writes to the stream with ctx. Taking it counts as
// using the stream. Close or abort the stream itself when done.
func (s *OutputStream) Writer(ctx context.Context) io.Writer {
	if o, err := s.get(); err == nil {
		o.mu.Lock()
		if o.usage == useUnused && !o.ended {
			o.usage = useUsed
		}
		o.mu.Unlock()
	}
	return &streamWriter{ctx: ctx, s: s}
}

type streamWriter struct {
	ctx context.Context
	s   *OutputStream
}

func (w *streamWriter) Write(p []byte) (int, error) {
	if err := w.s.Write(w.ctx, p); err != nil {
		return 0, err
	}
	return len(p), nil
}

// MarshalJSON writes the stream as the ref that takes its place in a payload or a
// result: its own ref when it was received and not yet used (it is forwarded), else a
// placeholder the SDK replaces with the ref of a new pipe. Marshal a stream only to
// send it: a marshalled stream counts as sent.
func (s OutputStream) MarshalJSON() ([]byte, error) {
	if s.inner == nil {
		return []byte("null"), nil
	}
	return s.inner.marshal()
}

// UnmarshalJSON reads a "write" ref as the stream it stands for (see
// [InputStream.UnmarshalJSON]). A "read" ref is an error: it is an [InputStream].
func (s *OutputStream) UnmarshalJSON(data []byte) error {
	if bytes.Equal(bytes.TrimSpace(data), []byte("null")) {
		return nil
	}
	ref, err := decodeStreamRef(data)
	if err != nil {
		return err
	}
	if ref.Direction != ChannelWrite {
		return errors.New(`iii: expected a stream to write (direction "write"): a "read" ref is an InputStream`)
	}
	if e := lookupReceived(receivedKey{ref.ChannelID, ChannelWrite}); e != nil && e.out != nil {
		if token, ok := placeholderToken(ref); ok {
			claimPlaceholder(token)
		}
		s.inner = e.out
		return nil
	}
	if token, ok := placeholderToken(ref); ok {
		e, ok := claimPlaceholder(token)
		if !ok || e.out == nil {
			if ok {
				registerPlaceholder(token, e)
			}
			return gone(ref.ChannelID)
		}
		s.inner = e.out
		return nil
	}
	s.inner = newOutput(&ref, nil, nil)
	return nil
}

// JSONSchema is the schema of a stream to write, for [RegisterFunctionTyped] and
// [InferSchema]: the ref object, marked with "x-iii-stream": "write".
func (OutputStream) JSONSchema() *jsonschema.Schema {
	return streamSchema("OutputStream", ChannelWrite,
		`A stream the function writes: the ref of the write end of an engine pipe ({channel_id, access_key, direction: "write"}). The SDK opens it on first write.`)
}

// =====================================================================================
// StreamInfo and Pipe
// =====================================================================================

// StreamInfo is a stream an observed call carried, as its observers see it: that a
// stream was passed, which way, and its channel id. It holds no bytes, and no access
// key.
//
// An observer's target receives it in place of each stream ref under the Input and
// Output of an [Observed] payload. The engine still sends the ref (access key included)
// to the observer's worker: the SDK hides it, it does not keep it off the wire.
type StreamInfo struct {
	// Stream is always true: it marks the value as a stream.
	Stream bool `json:"stream"`
	// Direction is "read" for a stream the function read, "write" for one it wrote.
	Direction ChannelDirection `json:"direction"`
	// ID is the channel id (a local stream's placeholder id for a call this worker
	// dispatched in-process).
	ID string `json:"id"`
}

// createPipe asks the engine for a pipe: its writer and reader refs.
func (c *Client) createPipe(ctx context.Context) (StreamChannelRef, StreamChannelRef, error) {
	// Straight to the engine: the pipe's refs are not a result to track.
	res, err := c.invokeRemote(ctx, CallRequest{
		FunctionID: FnCreateChannel,
		Payload:    json.RawMessage(`{"buffer_size":null}`),
	}, invocationNamespace(FnCreateChannel, "", c.namespace), nil)
	if err != nil {
		return StreamChannelRef{}, StreamChannelRef{}, err
	}
	var refs struct {
		Writer *StreamChannelRef `json:"writer"`
		Reader *StreamChannelRef `json:"reader"`
	}
	if err := json.Unmarshal(res, &refs); err != nil {
		return StreamChannelRef{}, StreamChannelRef{}, fmt.Errorf("iii: decoding the pipe refs: %w", err)
	}
	if refs.Writer == nil || refs.Reader == nil {
		return StreamChannelRef{}, StreamChannelRef{}, errors.New("iii: the engine answered engine::channels::create without a writer and a reader")
	}
	return *refs.Writer, *refs.Reader, nil
}

// Pipe creates a pipe on the engine and returns its two ends: put one in a call payload
// (or a handler result) and use the other here. This worker owns the pipe: it must stay
// connected until the other side attached to its end, or the engine drops it.
//
// Most code does not need it: a stream built with [InputStreamFromReader],
// [InputStreamFromBytes] or [OutputStreamToWriter] gets its pipe from the SDK. Use a
// pipe for a stream handed out before its data exists, a stream given to a call made
// later, a two-way session (two pipes), or a reader that needs the messages too (a sink
// takes bytes only).
//
//	input, output, err := worker.Pipe(ctx)
//	if err != nil { return err }
//	payload, _ := json.Marshal(map[string]any{"prompt": "Summarize report r-42", "tokens": output})
//	done := make(chan error, 1)
//	go func() {
//	    _, err := worker.Call(ctx, iii.CallRequest{FunctionID: "llm::complete", Payload: payload})
//	    done <- err
//	}()
//	for msg, err := range input.Messages(ctx) { ... }
//	return <-done
func (c *Client) Pipe(ctx context.Context) (*InputStream, *OutputStream, error) {
	writer, reader, err := c.createPipe(ctx)
	if err != nil {
		return nil, nil, err
	}
	ep := c.streamEndpoint()
	return &InputStream{inner: newInput(&reader, nil, ep)}, &OutputStream{inner: newOutput(&writer, nil, ep)}, nil
}
