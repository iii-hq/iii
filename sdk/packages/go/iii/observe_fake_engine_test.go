package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"log"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
)

// fakeEngine scripts the engine side of observe on top of mockEngine: it greets every
// connection with workerregistered (with or without observe.v1), answers each
// registerfunction with functionregistered (the function's current observer set), and
// lets a test push observeradded / observerremoved / observe results and reply to the
// worker's calls.
type fakeEngine struct {
	t *testing.T
	m *mockEngine

	mu            sync.Mutex
	features      []string
	confirm       bool
	localDispatch bool
	sets          map[string][]ObserverInfo
	revisions     map[string]uint64
	connects      int
	onFrame       func(msg map[string]json.RawMessage)
	// gate, when set, holds workerregistered on the next connections until closed.
	gate chan struct{}
}

func newFakeEngine(t *testing.T) *fakeEngine {
	t.Helper()
	fe := &fakeEngine{
		t:             t,
		m:             newMockEngine(t),
		features:      []string{FeatureObserveV1, FeatureLocalDispatchV1},
		confirm:       true,
		localDispatch: true,
		sets:          map[string][]ObserverInfo{},
		revisions:     map[string]uint64{},
	}
	fe.m.mu.Lock()
	fe.m.onConnect = fe.greet
	fe.m.onReceive = fe.receive
	fe.m.mu.Unlock()
	return fe
}

// oldEngine makes the engine a pre-0.25 one: no features, no functionregistered.
func (fe *fakeEngine) oldEngine() *fakeEngine {
	fe.mu.Lock()
	fe.features = nil
	fe.confirm = false
	fe.mu.Unlock()
	return fe
}

func (fe *fakeEngine) setOnFrame(fn func(msg map[string]json.RawMessage)) {
	fe.mu.Lock()
	fe.onFrame = fn
	fe.mu.Unlock()
}

func (fe *fakeEngine) greet(conn *websocket.Conn) {
	fe.mu.Lock()
	fe.connects++
	features, gate := fe.features, fe.gate
	fe.mu.Unlock()
	if gate != nil {
		<-gate
	}
	_ = fe.m.send(context.Background(), conn, &WorkerRegisteredMessage{WorkerID: "worker-1", ReattachToken: "token-1", Features: features})
}

func (fe *fakeEngine) receive(conn *websocket.Conn, msg map[string]json.RawMessage) {
	fe.mu.Lock()
	confirm, localDispatch, hook := fe.confirm, fe.localDispatch, fe.onFrame
	fe.mu.Unlock()
	if confirm && messageType(msg) == string(MsgRegisterFunction) {
		fe.confirmFunction(conn, messageID(msg), localDispatch)
	}
	if hook != nil {
		hook(msg)
	}
}

func (fe *fakeEngine) confirmFunction(conn *websocket.Conn, id string, localDispatch bool) {
	fe.mu.Lock()
	set := append([]ObserverInfo{}, fe.sets[id]...)
	rev := fe.revisions[id]
	fe.mu.Unlock()
	_ = fe.m.send(context.Background(), conn, &FunctionRegisteredMessage{
		ID: id, Namespace: "default", Revision: rev, LocalDispatch: localDispatch, Observers: set,
	})
}

// send writes a message to the current connection.
func (fe *fakeEngine) send(msg any) {
	fe.t.Helper()
	fe.m.mu.Lock()
	conn := fe.m.active
	fe.m.mu.Unlock()
	if conn == nil {
		fe.t.Fatalf("fake engine: no connection to send %T on", msg)
	}
	if err := fe.m.send(context.Background(), conn, msg); err != nil {
		fe.t.Fatalf("fake engine: send %T: %v", msg, err)
	}
}

func (fe *fakeEngine) sendRaw(frame string) {
	fe.t.Helper()
	fe.m.mu.Lock()
	conn := fe.m.active
	fe.m.mu.Unlock()
	if err := fe.m.sendRaw(context.Background(), conn, []byte(frame)); err != nil {
		fe.t.Fatalf("fake engine: send raw: %v", err)
	}
}

// add attaches an observer to function fn: it joins the set later
// functionregistered frames carry, and observeradded is pushed with the next revision.
func (fe *fakeEngine) add(fn string, info ObserverInfo) uint64 {
	fe.mu.Lock()
	fe.revisions[fn]++
	rev := fe.revisions[fn]
	fe.sets[fn] = append(fe.sets[fn], info)
	fe.mu.Unlock()
	fe.send(&ObserverAddedMessage{FunctionID: fn, Namespace: "default", Revision: rev, Observer: info})
	return rev
}

// remove detaches an observer and pushes observerremoved.
func (fe *fakeEngine) remove(fn, id string) uint64 {
	fe.mu.Lock()
	fe.revisions[fn]++
	rev := fe.revisions[fn]
	var kept []ObserverInfo
	for _, o := range fe.sets[fn] {
		if o.ID != id {
			kept = append(kept, o)
		}
	}
	fe.sets[fn] = kept
	fe.mu.Unlock()
	fe.send(&ObserverRemovedMessage{FunctionID: fn, Namespace: "default", Revision: rev, ID: id})
	return rev
}

// setObservers replaces the set the next functionregistered of fn carries.
func (fe *fakeEngine) setObservers(fn string, rev uint64, set ...ObserverInfo) {
	fe.mu.Lock()
	fe.sets[fn] = set
	fe.revisions[fn] = rev
	fe.mu.Unlock()
}

// client connects a client and waits until WorkerRegistered was applied.
func (fe *fakeEngine) client(opts ...Option) *Client {
	fe.t.Helper()
	opts = append([]Option{
		WithName("observe-test"),
		WithReconnectConfig(ReconnectConfig{InitialDelay: 5 * time.Millisecond, MaxDelay: 20 * time.Millisecond, BackoffMultiplier: 1, MaxRetries: -1}),
	}, opts...)
	c := New(fe.m.url, opts...)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		fe.t.Fatalf("Connect: %v", err)
	}
	fe.t.Cleanup(func() { _ = c.Close() })
	waitUntil(fe.t, "WorkerRegistered applied", c.registeredNow)
	return c
}

func (c *Client) registeredNow() bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.registeredOnConnection
}

// reconnect drops the socket and waits for the client to register again.
func (fe *fakeEngine) reconnect(c *Client) {
	fe.t.Helper()
	fe.mu.Lock()
	before := fe.connects
	fe.mu.Unlock()
	fe.m.closeActiveConnection()
	waitUntil(fe.t, "reconnect", func() bool {
		fe.mu.Lock()
		defer fe.mu.Unlock()
		return fe.connects > before
	})
	waitUntil(fe.t, "WorkerRegistered applied after reconnect", c.registeredNow)
}

// frames returns the recorded frames of one type, decoded.
func (fe *fakeEngine) frames(msgType MessageType) []*DecodedMessage {
	var out []*DecodedMessage
	for _, raw := range fe.m.receivedMessages() {
		if messageType(raw) != string(msgType) {
			continue
		}
		out = append(out, decodeRecorded(fe.t, raw))
	}
	return out
}

// waitFrames waits until n frames of msgType match pred and returns them.
func (fe *fakeEngine) waitFrames(msgType MessageType, n int, pred func(*DecodedMessage) bool) []*DecodedMessage {
	fe.t.Helper()
	var got []*DecodedMessage
	deadline := time.Now().Add(3 * time.Second)
	for {
		got = got[:0]
		for _, d := range fe.frames(msgType) {
			if pred == nil || pred(d) {
				got = append(got, d)
			}
		}
		if len(got) >= n || time.Now().After(deadline) {
			break
		}
		time.Sleep(2 * time.Millisecond)
	}
	if len(got) < n {
		fe.t.Fatalf("waited for %d %s frames, got %d", n, msgType, len(got))
	}
	return got
}

func (fe *fakeEngine) acks(fn string) []*ObserverAckMessage {
	var out []*ObserverAckMessage
	for _, d := range fe.frames(MsgObserverAck) {
		if d.ObserverAck.FunctionID == fn {
			out = append(out, d.ObserverAck)
		}
	}
	return out
}

// waitAck waits for the ack of observer id with revision rev.
func (fe *fakeEngine) waitAck(fn, id string, rev uint64) *ObserverAckMessage {
	fe.t.Helper()
	got := fe.waitFrames(MsgObserverAck, 1, func(d *DecodedMessage) bool {
		a := d.ObserverAck
		return a.FunctionID == fn && a.ID == id && a.Revision == rev
	})
	return got[0].ObserverAck
}

func (fe *fakeEngine) reportItems() []ObservedItem {
	var out []ObservedItem
	for _, d := range fe.frames(MsgInvocationsObserved) {
		out = append(out, d.InvocationsObserved.Items...)
	}
	return out
}

func (fe *fakeEngine) waitItems(n int) []ObservedItem {
	fe.t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for {
		items := fe.reportItems()
		if len(items) >= n {
			return items
		}
		if time.Now().After(deadline) {
			fe.t.Fatalf("waited for %d report items, got %d", n, len(items))
		}
		time.Sleep(2 * time.Millisecond)
	}
}

// countFrames counts frames sent after mark (an index into the recorded frames),
// except the types listed.
func (fe *fakeEngine) countFrames(mark int, except ...MessageType) int {
	n := 0
	for _, raw := range fe.m.receivedMessages()[mark:] {
		skip := false
		for _, t := range except {
			if messageType(raw) == string(t) {
				skip = true
			}
		}
		if !skip {
			n++
		}
	}
	return n
}

func (fe *fakeEngine) mark() int { return len(fe.m.receivedMessages()) }

func decodeRecorded(t *testing.T, raw map[string]json.RawMessage) *DecodedMessage {
	t.Helper()
	data, err := json.Marshal(raw)
	if err != nil {
		t.Fatalf("re-encode frame: %v", err)
	}
	d, err := UnmarshalMessage(data)
	if err != nil {
		t.Fatalf("decode frame %s: %v", data, err)
	}
	return d
}

func waitUntil(t *testing.T, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for !cond() {
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for %s", what)
		}
		time.Sleep(time.Millisecond)
	}
}

// observerInfo is an observer as the engine pushes it to the owner.
func observerInfo(id, target string, args, filters string, capture ...Capture) ObserverInfo {
	info := ObserverInfo{ID: id, Target: target, On: OnSuccess, Capture: capture}
	if capture == nil {
		info.Capture = []Capture{CaptureInput, CaptureOutput, CaptureCaller}
	}
	if args != "" {
		info.Args = json.RawMessage(args)
	} else {
		info.Args = json.RawMessage(`{}`)
	}
	if filters != "" {
		info.Filters = json.RawMessage(filters)
	} else {
		info.Filters = json.RawMessage(`{}`)
	}
	return info
}

// captureLog redirects the standard logger for the test and returns its buffer.
func captureLog(t *testing.T) *syncBuffer {
	t.Helper()
	buf := &syncBuffer{}
	prev, flags := log.Writer(), log.Flags()
	log.SetOutput(buf)
	t.Cleanup(func() { log.SetOutput(prev); log.SetFlags(flags) })
	return buf
}

type syncBuffer struct {
	mu  sync.Mutex
	buf bytes.Buffer
}

func (b *syncBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *syncBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.String()
}

func (b *syncBuffer) count(substr string) int { return strings.Count(b.String(), substr) }

func echoHandler(_ context.Context, data json.RawMessage) (any, error) { return data, nil }

func jsonEq(t *testing.T, got, want []byte) bool {
	t.Helper()
	var a, b any
	if err := json.Unmarshal(got, &a); err != nil {
		t.Fatalf("decode %s: %v", got, err)
	}
	if err := json.Unmarshal(want, &b); err != nil {
		t.Fatalf("decode %s: %v", want, err)
	}
	return deepEqualJSON(a, b)
}

func deepEqualJSON(a, b any) bool {
	ja, _ := json.Marshal(a)
	jb, _ := json.Marshal(b)
	return bytes.Equal(ja, jb)
}
