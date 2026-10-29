package iii

import (
	"context"
	"encoding/json"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// The reviewer's probe: an observe and the unobserve of the same (disposed) observer,
// meant for a connection that is gone, reach the next connection's writer. The
// observe is dropped at write time; the unobserve is re-routed and sent on the new
// connection once WorkerRegistered arrived, so the engine never keeps the observer.
func TestStaleObserveFramesAreDroppedAtWriteTime(t *testing.T) {
	fe := newFakeEngine(t)
	c := New(fe.m.url, WithName("observe-test"))
	t.Cleanup(func() { _ = c.Close() })
	h, _ := c.Observe(Source{FunctionID: "orders::create"}, "audit::log")
	h.markSent() // it was sent on the old connection
	c.mu.Lock()
	delete(c.observers, h.id) // and disposed while its frames were stuck
	c.mu.Unlock()
	unobserve, _ := prepareJSON(&UnobserveMessage{ID: h.id})
	c.observeOut <- observeFrame{data: h.frame, epoch: 41}
	c.observeOut <- observeFrame{data: unobserve, epoch: 41, unobserveID: h.id}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	fe.waitFrames(MsgUnobserve, 1, func(d *DecodedMessage) bool { return d.Unobserve.ID == h.id })
	time.Sleep(30 * time.Millisecond)
	if n := len(observeFrames(fe, h.id)); n != 0 {
		t.Errorf("a stale observe of a disposed observer was sent %d times", n)
	}
	msgs := fe.m.receivedMessages()
	if n := countType(msgs, string(MsgUnobserve)); n != 1 {
		t.Errorf("%d unobserves", n)
	}
}

// Report frames handed to a writer that never wrote them are counted DISCONNECTED.
func TestUnwrittenReportFramesAreCountedDisconnected(t *testing.T) {
	host := &fakeReportHost{online: true, status: reportReady}
	c := New("ws://unused.invalid")
	t.Cleanup(func() { _ = c.Close() })
	c.reporter = newReporter(host, defaultReporterLimits)
	frame, _ := MarshalMessage(&InvocationsObservedMessage{Items: []ObservedItem{
		{FunctionID: "a::fn", InvocationID: "1", Matched: []string{"o"}, Status: "success"},
		{FunctionID: "a::fn", InvocationID: "2", Matched: []string{"o"}, Status: "success"},
		{FunctionID: "b::fn", InvocationID: "3", Matched: []string{"o"}, Status: "success"},
	}})
	// the reporter builds frames with this exact head
	frame = append(append([]byte(nil), reportFrameHead...), frame[len(`{"items":[`):len(frame)-len(`],"type":"invocationsobserved"}`)]...)
	frame = append(frame, reportFrameTail...)
	reply := make(chan []byte, 4)
	reply <- frame
	reply <- []byte(`{"type":"pong"}`)
	c.discardUnwritten(reply, &sync.WaitGroup{})
	c.reporter.sendStats(time.Now())
	stats := lastStats(t, host)
	got := map[string]uint64{}
	for _, d := range stats.DroppedReports {
		got[d.FunctionID+"/"+d.Reason] = d.Count
	}
	if got["a::fn/DISCONNECTED"] != 2 || got["b::fn/DISCONNECTED"] != 1 {
		t.Errorf("drops %+v", stats.DroppedReports)
	}
}

// A written() between the budget read and the deferral is not lost: the deferred
// flush still runs.
func TestDeferredFlushIsNotLost(t *testing.T) {
	host := &racingQueueHost{fakeReportHost: fakeReportHost{online: true, status: reportReady}}
	r := newReporter(host, defaultReporterLimits)
	host.r = r
	r.push(testItem("f", 1), []Capture{CaptureInput})
	host.mu2.Lock()
	host.armed = true // from now on: the flush's own read races the writer
	host.mu2.Unlock()
	r.start()
	defer r.shutdown()
	r.kick()
	waitUntil(t, "the deferred flush", func() bool { return len(host.sentFrames()) == 1 })
}

// racingQueueHost reports a full socket on the first read, with the writer catching
// up right then (written() runs inside the read).
type racingQueueHost struct {
	fakeReportHost
	r     *reporter
	mu2   sync.Mutex
	armed bool
	calls int
}

func (h *racingQueueHost) reportQueued() int {
	h.mu2.Lock()
	if h.armed {
		h.calls++
	}
	first := h.armed && h.calls == 1
	h.mu2.Unlock()
	if first {
		h.r.written()
		return defaultReporterLimits.maxBytes
	}
	return 0
}

// An item larger than what is left of the budget waits while anything is in flight.
func TestOverBudgetItemWaitsWhileFramesAreInFlight(t *testing.T) {
	host := &fakeReportHost{online: true, status: reportReady}
	limits := defaultReporterLimits
	limits.maxBytes = 4096
	r := newReporter(host, limits)
	big := testItem("f", 1)
	big.values.input = json.RawMessage(`"` + strings.Repeat("x", 600) + `"`)
	if !r.push(big, []Capture{CaptureInput}) {
		t.Fatal("push")
	}
	host.mu.Lock()
	host.queued = limits.maxBytes - 100 // budget 100, the item is ~700 bytes
	host.mu.Unlock()
	r.flush()
	if len(host.sentFrames()) != 0 || r.size() != 1 {
		t.Fatalf("sent over the budget while frames are in flight: %d frames", len(host.sentFrames()))
	}
	host.mu.Lock()
	host.queued = 0
	host.mu.Unlock()
	r.flush()
	if len(host.sentFrames()) != 1 || r.size() != 0 {
		t.Errorf("not sent once idle: %d frames, %d items", len(host.sentFrames()), r.size())
	}
}

// SDK-PARITY decision 29: the handler gets its own copy of the metadata; what it does
// with it does not change the report.
func TestHandlerMetadataEditsDoNotReachTheReport(t *testing.T) {
	fe := newFakeEngine(t)
	fe.setObservers("md::fn", 1, observerInfo("o1", "x::y", "", "", CaptureMetadata))
	c := fe.client()
	registerLocal(t, c, "md::fn", func(ctx context.Context, _ json.RawMessage) (any, error) {
		md, _ := MetadataFromContext(ctx)
		copy(md[6:], "EDITED!!")
		return nil, nil
	})
	fe.waitAck("md::fn", "o1", 1)
	call(t, c, CallRequest{FunctionID: "md::fn", Metadata: json.RawMessage(`{"m":"original"}`)})
	if it := fe.waitItems(1)[0]; string(it.Metadata) != `{"m":"original"}` {
		t.Errorf("reported metadata %s", it.Metadata)
	}
}

// SDK-PARITY decision 31: a number no double can hold is reported as null (as the
// evaluator reads it); the engine's decoder would refuse it and fail the whole frame.
func TestOverflowingNumbersAreReportedAsNull(t *testing.T) {
	huge := strings.Repeat("9", 400)
	for in, want := range map[string]string{
		`{"a":1e400,"b":[` + huge + `,-` + huge + `],"c":"1e400","d":1.5,"e":1e308,"f":-2E+400}`: `{"a":null,"b":[null,null],"c":"1e400","d":1.5,"e":1e308,"f":null}`,
		`{"s":"a\"1e400\\","n":1e-400}`:               `{"s":"a\"1e400\\","n":1e-400}`,
		`[1,2.5,-0,` + strings.Repeat("1", 300) + `]`: `[1,2.5,-0,` + strings.Repeat("1", 300) + `]`,
		`1e400`: `null`,
	} {
		if got := string(overflowToNull(json.RawMessage(in))); got != want {
			t.Errorf("overflowToNull(%.60s...)\n got  %.120s\n want %.120s", in, got, want)
		}
	}
	plain := json.RawMessage(`{"a":1,"b":"x"}`)
	if got := overflowToNull(plain); &got[0] != &plain[0] {
		t.Error("a payload without overflow must be returned as is")
	}

	fe := newFakeEngine(t)
	fe.setObservers("big::fn", 1, observerInfo("o1", "x::y", "", "", CaptureInput, CaptureOutput, CaptureMetadata))
	c := fe.client()
	registerLocal(t, c, "big::fn", func(context.Context, json.RawMessage) (any, error) {
		return json.RawMessage(`{"out":-1e999}`), nil
	})
	fe.waitAck("big::fn", "o1", 1)
	call(t, c, CallRequest{FunctionID: "big::fn", Payload: json.RawMessage(`{"in":` + huge + `}`), Metadata: json.RawMessage(`{"m":1E400}`)})
	// an event source: the event is reported once, as input
	fe.setObservers("big::source", 1, observerInfo("e1", "x::y", "", "", CaptureInput, CaptureOutput))
	registerLocal(t, c, "big::source", Identity)
	fe.waitAck("big::source", "e1", 1)
	call(t, c, CallRequest{FunctionID: "big::source", Payload: json.RawMessage(`{"event":1e400}`)})
	items := fe.waitItems(2)
	for _, d := range fe.frames(MsgInvocationsObserved) {
		raw, _ := MarshalMessage(d.InvocationsObserved)
		var strict any // a float64 decoder refuses overflowing numbers, as serde_json does
		if err := json.Unmarshal(raw, &strict); err != nil {
			t.Errorf("a report frame a strict decoder refuses: %v", err)
		}
	}
	byFn := map[string]ObservedItem{}
	for _, it := range items {
		byFn[it.FunctionID] = it
	}
	local, source := byFn["big::fn"], byFn["big::source"]
	if string(local.Input) != `{"in":null}` || string(local.Output) != `{"out":null}` || string(local.Metadata) != `{"m":null}` {
		t.Errorf("local item: input %s output %s metadata %s", local.Input, local.Output, local.Metadata)
	}
	if string(source.Input) != `{"event":null}` || source.Output != nil || !source.OutputIsInput {
		t.Errorf("source item: input %s output %s output_is_input %v", source.Input, source.Output, source.OutputIsInput)
	}
}

// The reviewer's probe: what outbound holds (here filler and a registerfunction) is
// written before an observe frame, so a local target is registered before the
// observer that names it. Without the drain, the writer's select would pick the
// observe ahead of most of 60 queued frames.
func TestWriterDrainsOutboundBeforeAnObserve(t *testing.T) {
	fe := newFakeEngine(t)
	c := New(fe.m.url, WithName("observe-test"))
	t.Cleanup(func() { _ = c.Close() })
	for i := 0; i < 59; i++ {
		c.outbound <- []byte(`{"type":"pong"}`)
	}
	reg, _ := prepareJSON(&RegisterFunctionMessage{ID: "drain::target"})
	c.outbound <- reg
	obs, _ := prepareJSON(&ObserveMessage{ID: "drain-obs", Source: ObserveMessageSource{FunctionID: "a::b"}, Target: ObserveMessageTarget{FunctionID: "drain::target"}})
	c.observeOut <- observeFrame{data: obs, epoch: 1} // the first connection's epoch
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		t.Fatal(err)
	}
	fe.waitFrames(MsgObserve, 1, nil)
	msgs := fe.m.receivedMessages()
	regAt, obsAt := -1, -1
	for i, m := range msgs {
		if messageType(m) == string(MsgRegisterFunction) && messageID(m) == "drain::target" && regAt < 0 {
			regAt = i
		}
		if messageType(m) == string(MsgObserve) && messageID(m) == "drain-obs" {
			obsAt = i
		}
	}
	if regAt < 0 || obsAt < regAt || countType(msgs[:obsAt], string(MsgPong)) != 59 {
		t.Errorf("registerfunction at %d, observe at %d, %d fillers before it", regAt, obsAt, countType(msgs[:obsAt], string(MsgPong)))
	}
}

// A teardown while a report send is blocked on the connection's full reply channel
// returns, and every report is accounted for exactly once: the frame already in the
// channel is counted DISCONNECTED, the blocked one goes back to the buffer.
func TestTeardownWithABlockedReportSend(t *testing.T) {
	c, _ := wedgedClient(t)
	t.Cleanup(func() { _ = c.Close() })
	item, _ := encodeCompact(&ObservedItem{FunctionID: "w::fn", InvocationID: "in-channel", Matched: []string{"o"}, Status: "success"})
	full := make(chan []byte, 1)
	full <- append(append(append([]byte(nil), reportFrameHead...), item...), reportFrameTail...)
	c.replyMu.Lock()
	c.reply, c.replySenders, c.replyQueued = full, new(sync.WaitGroup), new(atomic.Int64)
	c.replyMu.Unlock()

	c.reporter.push(testItem("w::fn", 7), []Capture{CaptureInput})
	flushed := make(chan struct{})
	go func() {
		c.reporter.flush() // blocks in sendReport: the channel is full, nobody writes
		close(flushed)
	}()
	waitUntil(t, "the blocked send", func() bool {
		c.reporter.mu.Lock()
		defer c.reporter.mu.Unlock()
		return c.reporter.sending > 0
	})

	// the teardown of runConnection
	reply, done, senders := c.detachReply()
	close(done)
	torn := make(chan struct{})
	go func() {
		c.discardUnwritten(reply, senders)
		close(torn)
	}()
	select {
	case <-torn:
	case <-time.After(2 * time.Second):
		t.Fatal("the teardown waits for the blocked send forever")
	}
	<-flushed

	c.reporter.mu.Lock()
	defer c.reporter.mu.Unlock()
	if n := c.reporter.drops[dropKey{"w::fn", dropDisconnected}]; n != 1 {
		t.Errorf("DISCONNECTED %d, want 1 (the frame left in the channel, once)", n)
	}
	if len(c.reporter.items) != 1 || c.reporter.items[0].wire.InvocationID != "inv-7" || c.reporter.sending != 0 {
		t.Errorf("the blocked item: %d items buffered, sending %d", len(c.reporter.items), c.reporter.sending)
	}
}
