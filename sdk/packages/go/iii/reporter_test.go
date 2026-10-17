package iii

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"
)

func testItem(fn string, i int) reportInit {
	return reportInit{
		functionID: fn, invocationID: fmt.Sprintf("inv-%d", i), matched: []string{"o1"}, status: "success",
		values: reportValues{input: json.RawMessage(fmt.Sprintf(`{"i":%d}`, i))}, startedAtMs: 1, durationMs: 0.5,
	}
}

func decodeBatch(t *testing.T, frame []byte) []ObservedItem {
	t.Helper()
	d, err := UnmarshalMessage(frame)
	if err != nil || d.InvocationsObserved == nil {
		t.Fatalf("not an invocationsobserved frame: %s (%v)", frame, err)
	}
	return d.InvocationsObserved.Items
}

func lastStats(t *testing.T, h *fakeReportHost) *LocalInvocationStatsMessage {
	t.Helper()
	frames := h.sentFrames()
	for i := len(frames) - 1; i >= 0; i-- {
		if d, err := UnmarshalMessage(frames[i]); err == nil && d.LocalInvocationStats != nil {
			return d.LocalInvocationStats
		}
	}
	t.Fatal("no localinvocationstats sent")
	return nil
}

func TestReporterFlushesAt256ItemsInOneFrame(t *testing.T) {
	host := &fakeReportHost{online: true, status: reportReady}
	limits := defaultReporterLimits
	limits.linger = time.Hour
	r := newReporter(host, limits)
	r.start()
	defer r.shutdown()
	for i := 0; i < 256; i++ {
		r.push(testItem("f", i), []Capture{CaptureInput})
	}
	waitUntil(t, "a frame", func() bool { return len(host.sentFrames()) == 1 })
	if items := decodeBatch(t, host.sentFrames()[0]); len(items) != 256 || string(items[255].Input) != `{"i":255}` {
		t.Errorf("%d items", len(items))
	}
}

func TestReporterFlushesASmallBatchAfterTheLinger(t *testing.T) {
	host := &fakeReportHost{online: true, status: reportReady}
	r := newReporter(host, defaultReporterLimits)
	r.start()
	defer r.shutdown()
	start := time.Now()
	r.push(testItem("f", 1), []Capture{CaptureInput})
	r.push(testItem("f", 2), []Capture{CaptureInput})
	waitUntil(t, "a frame", func() bool { return len(host.sentFrames()) == 1 })
	if items := decodeBatch(t, host.sentFrames()[0]); len(items) != 2 {
		t.Errorf("%d items", len(items))
	}
	if time.Since(start) < defaultReporterLimits.linger {
		t.Error("flushed before the linger")
	}
}

// CONTRACT D12: bounded by items and bytes; drops the newest and counts it.
func TestReporterIsBoundedAndCountsDrops(t *testing.T) {
	host := &fakeReportHost{online: false}
	limits := defaultReporterLimits
	limits.maxItems = 3
	r := newReporter(host, limits)
	for i := 0; i < 5; i++ {
		ok := r.push(testItem("f", i), []Capture{CaptureInput})
		if ok != (i < 3) {
			t.Errorf("push %d: %v", i, ok)
		}
	}
	if r.size() != 3 || r.items[2].wire.InvocationID != "inv-2" {
		t.Fatalf("kept %d items (the newest are dropped)", r.size())
	}

	limits = defaultReporterLimits
	one := newReportItem(testItem("f", 0))
	one.capture([]Capture{CaptureInput})
	_ = one.render()
	limits.maxBytes = len(one.json)*2 + 1
	rb := newReporter(host, limits)
	rb.push(testItem("f", 1), []Capture{CaptureInput})
	rb.push(testItem("f", 2), []Capture{CaptureInput})
	if rb.push(testItem("f", 3), []Capture{CaptureInput}) {
		t.Error("over the byte bound")
	}
	huge := testItem("g", 9)
	huge.values.input = json.RawMessage(`"` + strings.Repeat("x", limits.maxBytes) + `"`)
	if rb.push(huge, []Capture{CaptureInput}) {
		t.Error("an item larger than the buffer was accepted")
	}
	host.set(true, reportReady)
	rb.sendStats(time.Now())
	stats := lastStats(t, host)
	got := map[string]uint64{}
	for _, d := range stats.DroppedReports {
		got[d.FunctionID+"/"+d.Reason] = d.Count
	}
	if got["f/BUFFER_FULL"] != 1 || got["g/TOO_LARGE"] != 1 {
		t.Errorf("drops %+v", stats.DroppedReports)
	}
}

func TestReporterSplitsAFlushUnderTheFrameLimit(t *testing.T) {
	host := &fakeReportHost{online: true, status: reportReady}
	limits := defaultReporterLimits
	one := newReportItem(testItem("f", 10))
	one.capture([]Capture{CaptureInput})
	_ = one.render()
	limits.maxFrameBytes = reportFrameOverhead + 3*len(one.json) + 2
	r := newReporter(host, limits)
	for i := 10; i < 17; i++ {
		r.push(testItem("f", i), []Capture{CaptureInput})
	}
	r.flush()
	frames := host.sentFrames()
	total := 0
	for _, f := range frames {
		if len(f) > limits.maxFrameBytes {
			t.Errorf("frame of %d bytes over %d", len(f), limits.maxFrameBytes)
		}
		total += len(decodeBatch(t, f))
	}
	if len(frames) != 3 || total != 7 {
		t.Errorf("%d frames, %d items", len(frames), total)
	}
}

// sdk.md §4.4: items stay while offline or until their function is confirmed on the
// connection; a function no longer registered drops them.
func TestReporterHoldsItemsUntilTheirFunctionIsConfirmed(t *testing.T) {
	host := &fakeReportHost{online: false, statuses: map[string]reportStatus{"ready": reportReady, "wait": reportWait, "gone": reportGone}}
	r := newReporter(host, defaultReporterLimits)
	for _, fn := range []string{"ready", "wait", "gone"} {
		r.push(testItem(fn, 1), []Capture{CaptureInput})
	}
	r.flush()
	if len(host.sentFrames()) != 0 || r.size() != 3 {
		t.Fatalf("offline: sent %d, kept %d", len(host.sentFrames()), r.size())
	}
	host.set(true, reportReady)
	r.flush()
	if items := decodeBatch(t, host.sentFrames()[0]); len(items) != 1 || items[0].FunctionID != "ready" || r.size() != 1 {
		t.Fatalf("online: sent %v, kept %d", items, r.size())
	}
	host.mu.Lock()
	host.statuses["wait"] = reportReady
	host.mu.Unlock()
	r.flush()
	if items := decodeBatch(t, host.sentFrames()[1]); len(items) != 1 || items[0].FunctionID != "wait" || r.size() != 0 {
		t.Fatalf("confirmed: sent %v, kept %d", items, r.size())
	}
}

// localinvocationstats goes out only when non-zero, and counts keep accumulating
// while offline.
func TestReporterStatsOnlyWhenNonZero(t *testing.T) {
	host := &fakeReportHost{online: false}
	r := newReporter(host, defaultReporterLimits)
	if r.sendStats(time.Now()) {
		t.Fatal("sent with nothing to report")
	}
	r.recordLocal("f", true, 1.5)
	if r.sendStats(time.Now()) {
		t.Fatal("sent while offline")
	}
	r.recordLocal("f", false, 2)
	host.set(true, reportReady)
	if !r.sendStats(time.Now()) {
		t.Fatal("not sent")
	}
	s := lastStats(t, host)
	if len(s.Functions) != 1 || s.Functions[0].Count != 2 || s.Functions[0].Errors != 1 || s.Functions[0].DurationMsSum != 3.5 {
		t.Errorf("stats %+v", s.Functions)
	}
	if r.sendStats(time.Now()) {
		t.Error("sent again with nothing new")
	}

	// the 10 s timer
	limits := defaultReporterLimits
	limits.statsInterval = 20 * time.Millisecond
	timed := newReporter(host, limits)
	timed.start()
	defer timed.shutdown()
	timed.recordLocal("g", true, 1)
	waitUntil(t, "stats on the timer", func() bool { return len(host.sentFrames()) == 2 })
}

func TestReporterCoalescesOnlyTheSameEventFromTheSameInvocation(t *testing.T) {
	host := &fakeReportHost{online: false}
	r := newReporter(host, defaultReporterLimits)
	a, b := &invocation{}, &invocation{}
	event := json.RawMessage(`{"row":1}`)
	init := reportInit{functionID: "db::on", invocationID: "i", status: "success", values: reportValues{input: event}}
	r.pushEmit(init, []Capture{CaptureInput}, "o1", event, a)
	r.pushEmit(init, []Capture{CaptureInput}, "o2", event, a) // coalesced
	r.pushEmit(init, []Capture{CaptureInput}, "o1", event, a) // the same observer again: a new item
	r.pushEmit(init, []Capture{CaptureInput}, "o3", event, b) // another invocation
	other := json.RawMessage(`{"row":2}`)
	r.pushEmit(reportInit{functionID: "db::on", invocationID: "i", status: "success", values: reportValues{input: other}}, nil, "o2", other, a)
	if r.size() != 4 || len(r.items[0].wire.Matched) != 2 {
		t.Errorf("%d items; first matched %v", r.size(), r.items[0].wire.Matched)
	}
}

// SDK-PARITY decision 21: frames sent and not yet written to the socket count against
// the bound: new items are dropped as BUFFER_FULL and the flush is deferred until the
// writer catches up.
func TestReporterBoundCoversUnwrittenFrames(t *testing.T) {
	host := &fakeReportHost{online: true, status: reportReady}
	limits := defaultReporterLimits
	limits.maxBytes = 4096
	r := newReporter(host, limits)
	if !r.push(testItem("f", 1), []Capture{CaptureInput}) {
		t.Fatal("push")
	}
	host.mu.Lock()
	host.queued = limits.maxBytes - 10 // the socket is not keeping up
	host.mu.Unlock()
	if r.push(testItem("f", 2), []Capture{CaptureInput}) {
		t.Error("accepted over the bound that includes unwritten frames")
	}
	host.mu.Lock()
	host.queued = limits.maxBytes
	host.mu.Unlock()
	r.flush()
	if len(host.sentFrames()) != 0 || r.size() != 1 || !r.deferred.Load() {
		t.Fatalf("flushed behind a full socket: %d frames, %d items", len(host.sentFrames()), r.size())
	}
	r.start()
	defer r.shutdown()
	host.mu.Lock()
	host.queued = 0
	host.mu.Unlock()
	r.written() // the writer caught up
	waitUntil(t, "the deferred flush", func() bool { return len(host.sentFrames()) == 1 })
	host.set(true, reportReady)
	r.sendStats(time.Now())
	stats := lastStats(t, host)
	if len(stats.DroppedReports) != 1 || stats.DroppedReports[0].Reason != "BUFFER_FULL" {
		t.Errorf("drops %+v", stats.DroppedReports)
	}
}

// SDK-PARITY decision 22: held items of a function that cannot be confirmed are
// discarded and counted as INACTIVE.
func TestReporterDiscardsTheHeldItemsOfAConflictedFunction(t *testing.T) {
	host := &fakeReportHost{online: true, status: reportWait}
	r := newReporter(host, defaultReporterLimits)
	r.push(testItem("conflicted", 1), []Capture{CaptureInput})
	r.push(testItem("conflicted", 2), []Capture{CaptureInput})
	r.push(testItem("other", 3), []Capture{CaptureInput})
	r.discardFunction("conflicted")
	if r.size() != 1 || r.items[0].wire.FunctionID != "other" {
		t.Fatalf("kept %d items", r.size())
	}
	r.sendStats(time.Now())
	s := lastStats(t, host)
	if len(s.DroppedReports) != 1 || s.DroppedReports[0].Reason != "INACTIVE" || s.DroppedReports[0].Count != 2 {
		t.Errorf("drops %+v", s.DroppedReports)
	}
}

// Items of a function no longer registered here are dropped and counted INACTIVE.
func TestReporterCountsGoneItemsAsInactive(t *testing.T) {
	host := &fakeReportHost{online: true, statuses: map[string]reportStatus{"gone": reportGone, "ok": reportReady}}
	r := newReporter(host, defaultReporterLimits)
	r.push(testItem("gone", 1), []Capture{CaptureInput})
	r.push(testItem("gone", 2), []Capture{CaptureInput})
	r.push(testItem("ok", 3), []Capture{CaptureInput})
	r.flush()
	if r.size() != 0 || len(decodeBatch(t, host.sentFrames()[0])) != 1 {
		t.Fatalf("kept %d", r.size())
	}
	r.sendStats(time.Now())
	s := lastStats(t, host)
	if len(s.DroppedReports) != 1 || s.DroppedReports[0].FunctionID != "gone" || s.DroppedReports[0].Reason != "INACTIVE" || s.DroppedReports[0].Count != 2 {
		t.Errorf("drops %+v", s.DroppedReports)
	}
}

// A frame that cannot be sent (the connection closed under the flush) puts its items
// back, in order, for the next connection.
func TestReporterPutsBackItemsOfAFrameThatCouldNotBeSent(t *testing.T) {
	host := &failingReportHost{fakeReportHost: fakeReportHost{online: true, status: reportReady}, fail: true}
	r := newReporter(host, defaultReporterLimits)
	for i := 0; i < 3; i++ {
		r.push(testItem("f", i), []Capture{CaptureInput})
	}
	r.flush()
	if r.size() != 3 || r.items[0].wire.InvocationID != "inv-0" {
		t.Fatalf("after a failed send: %d items", r.size())
	}
	host.mu.Lock()
	host.fail = false
	host.mu.Unlock()
	r.flush()
	if r.size() != 0 || len(decodeBatch(t, host.sentFrames()[0])) != 3 {
		t.Errorf("after the retry: %d items, %d frames", r.size(), len(host.sentFrames()))
	}
}

type failingReportHost struct {
	fakeReportHost
	fail bool
}

func (h *failingReportHost) sendReport(epoch uint64, frame []byte) bool {
	h.mu.Lock()
	fail := h.fail
	h.mu.Unlock()
	if fail {
		return false
	}
	return h.fakeReportHost.sendReport(epoch, frame)
}
