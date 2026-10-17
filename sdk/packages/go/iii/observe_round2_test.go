package iii

import (
	"context"
	"fmt"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// wedgedClient is a client "connected" to a writer that never drains: an unbuffered
// reply channel and an outbound channel nobody reads.
func wedgedClient(t *testing.T) (*Client, *functionObservers) {
	t.Helper()
	c := New("ws://unused.invalid")
	c.mu.Lock()
	c.state, c.registeredOnConnection, c.epoch = StateConnected, true, 1
	c.engineFeatures[FeatureObserveV1] = true
	f := newFunctionObservers(c, "w::fn", nil, true)
	f.confirmedEpoch, f.syncedEpoch = 1, 1
	c.functions["w::fn"] = registeredFunction{message: &RegisterFunctionMessage{ID: "w::fn"}, handler: echoHandler, observe: f}
	c.mu.Unlock()
	c.replyMu.Lock()
	c.reply, c.replyEpoch, c.replyDone, c.replyQueued = make(chan []byte), 1, make(chan struct{}), new(atomic.Int64)
	c.replyMu.Unlock()
	return c, f
}

// Close is bounded when the writer does not drain (a peer that stopped reading): the
// final flush is best effort.
func TestCloseIsBoundedWhenTheWriterIsWedged(t *testing.T) {
	c, _ := wedgedClient(t)
	c.reporter.push(testItem("w::fn", 1), []Capture{CaptureInput})
	c.reporter.recordLocal("w::fn", true, 1)
	start := time.Now()
	done := make(chan struct{})
	go func() {
		_ = c.Close()
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(3 * time.Second):
		t.Fatal("Close hangs behind a wedged writer")
	}
	if d := time.Since(start); d > time.Second {
		t.Errorf("Close took %s", d)
	}
}

// An unobserve never blocks the caller (nor, through observeMu, the read loop) behind
// a writer that does not drain: the observe pump does.
func TestDisposeDoesNotBlockOnAWedgedWriter(t *testing.T) {
	c, _ := wedgedClient(t)
	t.Cleanup(func() { _ = c.Close() })
	for len(c.outbound) < cap(c.outbound) {
		c.outbound <- []byte(`{}`)
	}
	var handles []*ObserverHandle
	for i := 0; i < 3; i++ {
		h, err := c.Observe(Source{FunctionID: "a::b"}, "c::d")
		if err != nil {
			t.Fatal(err)
		}
		handles = append(handles, h)
	}
	done := make(chan struct{})
	go func() {
		for _, h := range handles {
			_ = h.Dispose(context.Background())
		}
		c.observeMu.Lock() // what the read loop takes on WorkerRegistered
		c.observeMu.Unlock()
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("Dispose (or observeMu) blocked behind the writer")
	}
}

// An observe is sent only on the registered connection it was meant for: a stale epoch
// (a reconnect raced the call) leaves it to the new connection's replay.
func TestObserveIsNotSentOnAConnectionThatIsNotRegistered(t *testing.T) {
	c, _ := wedgedClient(t)
	t.Cleanup(func() { _ = c.Close() })
	c.mu.Lock()
	c.registeredOnConnection = false // a new connection, WorkerRegistered not yet here
	c.mu.Unlock()
	h, _ := c.Observe(Source{FunctionID: "a::b"}, "c::d")
	c.mu.Lock()
	c.registeredOnConnection = true
	c.mu.Unlock()
	c.sendObserve(h, 0) // the epoch of a connection that is gone
	time.Sleep(20 * time.Millisecond)
	if h.wasSent() || len(c.outbound) != 0 {
		t.Errorf("sent %v, %d frames queued", h.wasSent(), len(c.outbound))
	}
}

// SDK-PARITY decision 21: the bytes of a flushed batch count against the bound until
// the writer took them.
func TestReporterCountsBatchesInHandOff(t *testing.T) {
	host := &blockingReportHost{fakeReportHost: fakeReportHost{online: true, status: reportReady}, release: make(chan struct{}), entered: make(chan struct{}, 1)}
	one := newReportItem(testItem("f", 1))
	one.capture([]Capture{CaptureInput})
	_ = one.render()
	limits := defaultReporterLimits
	limits.maxBytes = 2*len(one.json) + reportFrameOverhead + len(one.json)/2
	r := newReporter(host, limits)
	r.push(testItem("f", 1), []Capture{CaptureInput})
	r.push(testItem("f", 2), []Capture{CaptureInput})
	flushed := make(chan struct{})
	go func() {
		r.flush()
		close(flushed)
	}()
	<-host.entered // the batch left the buffer and waits for the writer
	if r.size() != 0 {
		t.Fatalf("%d items still buffered", r.size())
	}
	if r.push(testItem("f", 3), []Capture{CaptureInput}) {
		t.Error("accepted over the bound while a batch is being handed off")
	}
	close(host.release)
	<-flushed
	if !r.push(testItem("f", 4), []Capture{CaptureInput}) {
		t.Error("refused after the hand-off")
	}
}

type blockingReportHost struct {
	fakeReportHost
	release chan struct{}
	entered chan struct{}
}

func (h *blockingReportHost) sendReport(epoch uint64, frame []byte) bool {
	select {
	case h.entered <- struct{}{}:
	default:
	}
	<-h.release
	return h.fakeReportHost.sendReport(epoch, frame)
}

// A FUNCTION_NAMESPACE_CONFLICT while an observer's add is running: once the hook
// returns, the observer is treated as removed (OnObserverRemoved, no success ack).
func TestConflictDuringAnAddRemovesTheObserver(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	entered, release := make(chan struct{}), make(chan struct{})
	var log hookLog
	registerOwner(t, fe, c, "cf::fn", &ObserveHooks{
		Delivery: DeliveryOwner,
		OnObserverAdded: func(_ context.Context, obs *Observer) error {
			log.add(obs.ID)
			close(entered)
			<-release
			return nil
		},
		OnObserverRemoved: func(_ context.Context, obs *Observer) { log.remove(obs.ID) },
	})
	fe.add("cf::fn", observerInfo("o1", "x::y", "", ""))
	<-entered
	fe.send(&RegistrationRejectedMessage{Code: "FUNCTION_NAMESPACE_CONFLICT", Namespace: "default", FunctionID: "cf::fn", OwnerWorkerID: "other"})
	waitUntil(t, "released", func() bool { return !c.LocalDispatchActive("cf::fn") })
	close(release)
	waitUntil(t, "OnObserverRemoved", func() bool { _, r := log.snapshot(); return len(r) == 1 })
	time.Sleep(20 * time.Millisecond)
	if c.IsObserved("cf::fn") {
		t.Error("the observer is active after the conflict")
	}
	for _, a := range fe.acks("cf::fn") {
		if a.ID == "o1" && a.Error == nil {
			t.Error("a success ack for an observer of a refused function")
		}
	}
}

// A Reject racing the end of OnObserverAdded is never lost: the observer ends
// inactive, rejected.
func TestRejectRacingTheEndOfTheHookIsNotLost(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	registerOwner(t, fe, c, "rr2::fn", &ObserveHooks{
		Delivery: DeliveryOwner,
		OnObserverAdded: func(_ context.Context, obs *Observer) error {
			go obs.Reject("GONE", "rejected right as the hook returns")
			return nil
		},
	})
	for i := 0; i < 30; i++ {
		id := fmt.Sprintf("o%d", i)
		fe.add("rr2::fn", observerInfo(id, "x::y", "", ""))
		waitUntil(t, "the rejection of "+id, func() bool {
			for _, a := range fe.acks("rr2::fn") {
				if a.ID == id && a.Error != nil && a.Error.Code == "GONE" {
					return true
				}
			}
			return false
		})
	}
	waitUntil(t, "nothing active", func() bool { return !c.IsObserved("rr2::fn") })
}

// The SDK keeps its own copy of what it reads from an Observer: an owner changing the
// exported fields (even concurrently) does not change delivery (go test -race).
func TestOwnerMutatingObserverFieldsChangesNothing(t *testing.T) {
	fe := newFakeEngine(t)
	c := fe.client()
	owner := newCronOwner(t, fe, c, "mut::fn")
	addCronObserver(t, fe, "mut::fn", "o1", "x", "", CaptureInput)
	obs := owner.observers("x")[0]
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		for i := 0; i < 200; i++ {
			obs.Capture = []Capture{CaptureMetadata}
			obs.Target = "elsewhere"
			obs.ID = "renamed"
			obs.Args = nil
		}
	}()
	for i := 0; i < 50; i++ {
		if ok, err := obs.Emit(context.Background(), map[string]int{"n": i}); !ok || err != nil {
			t.Fatalf("Emit: %v %v", ok, err)
		}
	}
	wg.Wait()
	items := fe.waitItems(1)
	for _, it := range items {
		if it.Matched[0] != "o1" || it.Input == nil {
			t.Fatalf("item %+v: the owner's changes reached delivery", it)
		}
	}
}

// A listener unsubscribed before a queued change reaches it is skipped.
func TestUnsubscribedListenerIsSkipped(t *testing.T) {
	h := newObserverHandle(New("ws://unused.invalid"), "id", nil, 1)
	entered, release := make(chan struct{}), make(chan struct{})
	h.OnStateChange(func(ObserverState, *ObserveReason) {
		select {
		case entered <- struct{}{}:
		default:
		}
		<-release
	})
	var calls atomic.Int32
	off := h.OnStateChange(func(ObserverState, *ObserveReason) { calls.Add(1) })
	h.setState(ObserverActive, nil)
	h.setState(ObserverSuspended, nil)
	<-entered // the first listener holds the first change
	off()
	close(release)
	time.Sleep(30 * time.Millisecond)
	if n := calls.Load(); n != 0 {
		t.Errorf("an unsubscribed listener ran %d times", n)
	}
}
