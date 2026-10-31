package iii

import (
	"bytes"
	"encoding/json"
	"log"
	"math"
	"strconv"
	"sync"
	"sync/atomic"
	"time"
)

// This file is the reporter: one bounded buffer for the deliveries of local calls,
// flushed as invocationsobserved batches, and the periodic localinvocationstats. Routed
// calls are never reported: the engine delivers them.
//
// It never blocks a call: pushing only appends under a mutex. When the buffer is full
// the newest item is dropped and counted (BUFFER_FULL); an item too large on its own is
// dropped and counted (TOO_LARGE). Items stay buffered while disconnected and are
// flushed once the engine confirmed their function on the new connection; items handed
// to a connection that closed before writing them are counted DISCONNECTED. Flushing
// and sending run on the reporter's own goroutine.
//
// Lock order: reporter.mu, then Client.mu, then functionObservers.mu. The client never
// calls into the reporter while holding its own locks.

type reportStatus int

const (
	// reportReady: the engine confirmed the function on the current connection.
	reportReady reportStatus = iota
	// reportWait: not yet (the item stays buffered).
	reportWait
	// reportGone: the function is no longer registered here (the item is dropped).
	reportGone
)

type reporterHost interface {
	// reportOnline is the current connection's epoch, whether it is connected to an
	// engine that advertised observe.v1, and whether that engine advertised observe.v2
	// (items may then set output_is_input).
	reportOnline() (epoch uint64, ok bool, v2 bool)
	reportStatus(functionID string, epoch uint64) reportStatus
	// sendReport sends one frame on connection epoch; false when it is gone.
	sendReport(epoch uint64, frame []byte) bool
	// reportQueued is the bytes of report frames sent and not yet written to the
	// socket: they count against the buffer's bound (SDK-PARITY decision 21).
	reportQueued() int
}

type reporterLimits struct {
	batchItems    int           // flush at this many items (256)
	linger        time.Duration // flush this long after the first buffered item (5 ms)
	maxItems      int           // bounded buffer: items (10 000)
	maxBytes      int           // bounded buffer: bytes (8 MiB); a larger item is TOO_LARGE
	maxFrameBytes int           // one frame's limit (16 MiB, the engine's)
	statsInterval time.Duration // localinvocationstats cadence (10 s)
}

var defaultReporterLimits = reporterLimits{
	batchItems:    256,
	linger:        5 * time.Millisecond,
	maxItems:      10_000,
	maxBytes:      8 * 1024 * 1024,
	maxFrameBytes: maxJSONFrameBytes,
	statsInterval: 10 * time.Second,
}

const (
	dropBufferFull   = "BUFFER_FULL"
	dropTooLarge     = "TOO_LARGE"
	dropInactive     = "INACTIVE"
	dropDisconnected = "DISCONNECTED"
)

var (
	reportFrameHead = []byte(`{"type":"` + string(MsgInvocationsObserved) + `","items":[`)
	reportFrameTail = []byte(`]}`)
)

var reportFrameOverhead = len(reportFrameHead) + len(reportFrameTail)

// reportValues are the parts an item may carry, as JSON; a part is sent only when some
// matched observer captures it. A nil part does not exist (no output on a failure).
// caller is the caller as items carry it (reportCaller).
type reportValues struct {
	input, output, errBody, metadata json.RawMessage
	caller                           *Caller
}

type reportInit struct {
	functionID   string
	namespace    string
	invocationID string
	matched      []string
	status       string
	values       reportValues
	startedAtMs  int64
	durationMs   float64
	observeDepth uint32
	trace        traceContext
}

// reportItem is the JSON of one delivery, encoded when it is built: it keeps no value
// of the call, so what the call's caller does with its result does not change it.
type reportItem struct {
	functionID   string
	invocationID string
	json         []byte
	// v2 is the form json is in: for an engine with observe.v2, an item that carries an
	// output equal to its input (as bytes) carries the input once, with
	// output_is_input.
	v2 bool
}

// newReportItem encodes the delivery init, with the parts captures asks for, for an
// engine with (v2) or without observe.v2.
func newReportItem(init reportInit, captures []Capture, v2 bool) (*reportItem, error) {
	durationMs := init.durationMs
	if math.IsNaN(durationMs) || math.IsInf(durationMs, 0) || durationMs < 0 {
		durationMs = 0
	}
	wire := ObservedItem{
		FunctionID:   init.functionID,
		Namespace:    init.namespace,
		InvocationID: init.invocationID,
		Matched:      init.matched,
		Status:       init.status,
		StartedAtMs:  init.startedAtMs,
		DurationMs:   durationMs,
		ObserveDepth: init.observeDepth,
		Traceparent:  init.trace.traceparent,
		Baggage:      init.trace.baggage,
	}
	// Numbers no double can hold become null, as the evaluator reads them: the engine's
	// decoder refuses them, which would fail the whole frame (SDK-PARITY decision 31).
	values := init.values
	for _, c := range captures {
		switch c {
		case CaptureInput:
			wire.Input = overflowToNull(values.input)
		case CaptureOutput:
			wire.Output = overflowToNull(values.output)
		case CaptureError:
			wire.Error = values.errBody
		case CaptureCaller:
			wire.Caller = values.caller
		case CaptureMetadata:
			wire.Metadata = overflowToNull(values.metadata)
		}
	}
	it := &reportItem{functionID: init.functionID, invocationID: init.invocationID}
	if err := it.encode(&wire, v2); err != nil {
		return nil, err
	}
	return it, nil
}

// encode sets the item's JSON to wire in the form for an engine with (v2) or without
// observe.v2.
func (it *reportItem) encode(wire *ObservedItem, v2 bool) error {
	switch {
	case !v2 && wire.OutputIsInput:
		wire.Output, wire.OutputIsInput = wire.Input, false
	case v2 && wire.Input != nil && wire.Output != nil && bytes.Equal(wire.Input, wire.Output):
		wire.Output, wire.OutputIsInput = nil, true
	}
	data, err := encodeCompact(wire)
	if err != nil {
		return err
	}
	it.json, it.v2 = data, v2
	return nil
}

// render encodes the item again, from its own JSON, for an engine with (v2) or without
// observe.v2.
func (it *reportItem) render(v2 bool) error {
	var wire ObservedItem
	if err := json.Unmarshal(it.json, &wire); err != nil {
		return err
	}
	return it.encode(&wire, v2)
}

// overflowToNull rewrites the number tokens of raw JSON whose value overflows a
// double (1e400, a 400-digit integer) as null. It returns raw itself when there is
// none, which is the common case: one pass, no allocation.
func overflowToNull(raw json.RawMessage) json.RawMessage {
	var out []byte
	last := 0
	inString := false
	for i := 0; i < len(raw); i++ {
		c := raw[i]
		if inString {
			switch c {
			case '\\':
				i++
			case '"':
				inString = false
			}
			continue
		}
		if c == '"' {
			inString = true
			continue
		}
		if c != '-' && (c < '0' || c > '9') {
			continue
		}
		start := i
		long := false
		for i < len(raw) && (raw[i] == '-' || raw[i] == '+' || raw[i] == '.' || raw[i] == 'e' || raw[i] == 'E' || (raw[i] >= '0' && raw[i] <= '9')) {
			if raw[i] == 'e' || raw[i] == 'E' {
				long = true
			}
			i++
		}
		token := raw[start:i]
		i-- // the loop's i++ moves past the token
		if !long && len(token) <= 300 {
			continue // at most 300 digits: below MaxFloat64 (1.8e308)
		}
		if f, err := strconv.ParseFloat(string(token), 64); err == nil || !math.IsInf(f, 0) {
			continue
		}
		out = append(out, raw[last:start]...)
		out = append(out, "null"...)
		last = start + len(token)
	}
	if out == nil {
		return raw
	}
	return append(out, raw[last:]...)
}

// encodeCompact is json.Marshal without HTML escaping.
func encodeCompact(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimRight(buf.Bytes(), "\n"), nil
}

type functionStats struct {
	count         uint64
	errors        uint64
	durationMsSum float64
}

type dropKey struct{ functionID, reason string }

type reporter struct {
	host   reporterHost
	limits reporterLimits

	mu    sync.Mutex
	items []*reportItem
	bytes int
	// sending is the bytes of flushed items not yet handed to the connection's
	// writer: they still count against the bound (SDK-PARITY decision 21).
	sending     int
	lingerTimer *time.Timer
	stats       map[string]*functionStats
	statsOrder  []string
	drops       map[dropKey]uint64
	dropsOrder  []dropKey
	windowStart time.Time
	unencodable map[string]bool

	// deferred: a flush stopped because the socket was not keeping up; the writer
	// wakes the reporter once it wrote a report frame.
	deferred atomic.Bool

	wake     chan struct{}
	stop     chan struct{}
	stopOnce sync.Once
	runOnce  sync.Once
}

func newReporter(host reporterHost, limits reporterLimits) *reporter {
	return &reporter{
		host:        host,
		limits:      limits,
		stats:       map[string]*functionStats{},
		drops:       map[dropKey]uint64{},
		windowStart: time.Now(),
		unencodable: map[string]bool{},
		wake:        make(chan struct{}, 1),
		stop:        make(chan struct{}),
	}
}

// start runs the flush / stats goroutine (once).
func (r *reporter) start() {
	r.runOnce.Do(func() {
		r.mu.Lock()
		r.windowStart = time.Now()
		r.mu.Unlock()
		go r.run()
	})
}

func (r *reporter) shutdown() {
	r.stopOnce.Do(func() {
		close(r.stop)
		r.mu.Lock()
		if r.lingerTimer != nil {
			r.lingerTimer.Stop()
			r.lingerTimer = nil
		}
		r.mu.Unlock()
	})
}

func (r *reporter) run() {
	ticker := time.NewTicker(r.limits.statsInterval)
	defer ticker.Stop()
	for {
		select {
		case <-r.wake:
			r.flush()
		case now := <-ticker.C:
			r.sendStats(now)
		case <-r.stop:
			return
		}
	}
}

// kick asks the reporter goroutine to flush; it never blocks.
func (r *reporter) kick() {
	select {
	case r.wake <- struct{}{}:
	default:
	}
}

// size is the number of buffered items (tests and diagnostics).
func (r *reporter) size() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.items)
}

// build makes the item of one delivery, in the form for the current engine (with
// observe.v2 while offline: the flush renders it again for an engine without), or nil
// when it cannot be encoded.
func (r *reporter) build(init reportInit, captures []Capture) *reportItem {
	_, online, v2 := r.host.reportOnline()
	item, err := newReportItem(init, captures, v2 || !online)
	if err != nil {
		r.logUnencodable(init.functionID, err)
		return nil
	}
	return item
}

// push buffers the delivery of a local call.
func (r *reporter) push(init reportInit, captures []Capture) bool {
	item := r.build(init, captures)
	if item == nil {
		return false
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.enqueueLocked(item)
}

func (r *reporter) logUnencodable(functionID string, err error) {
	r.mu.Lock()
	warned := r.unencodable[functionID]
	r.unencodable[functionID] = true
	r.mu.Unlock()
	if !warned {
		log.Printf("iii: cannot encode an observed call of %q as JSON; it is not reported: %v", functionID, err)
	}
}

func (r *reporter) enqueueLocked(item *reportItem) bool {
	fn := item.functionID
	size := len(item.json)
	if size > r.limits.maxBytes || size+reportFrameOverhead > r.limits.maxFrameBytes {
		r.dropLocked(fn, dropTooLarge, 1)
		return false
	}
	if len(r.items) >= r.limits.maxItems || r.bytes+r.sending+size+r.host.reportQueued() > r.limits.maxBytes {
		r.dropLocked(fn, dropBufferFull, 1)
		return false
	}
	r.items = append(r.items, item)
	r.bytes += size
	if len(r.items) >= r.limits.batchItems {
		r.kick()
	} else if r.lingerTimer == nil {
		r.lingerTimer = time.AfterFunc(r.limits.linger, r.kick)
	}
	return true
}

func (r *reporter) dropLocked(functionID, reason string, n uint64) {
	k := dropKey{functionID, reason}
	if _, ok := r.drops[k]; !ok {
		r.dropsOrder = append(r.dropsOrder, k)
	}
	r.drops[k] += n
}

// recordLocal counts one local invocation for localinvocationstats.
func (r *reporter) recordLocal(functionID string, ok bool, durationMs float64) {
	r.mu.Lock()
	defer r.mu.Unlock()
	s := r.stats[functionID]
	if s == nil {
		s = &functionStats{}
		r.stats[functionID] = s
		r.statsOrder = append(r.statsOrder, functionID)
	}
	s.count++
	if !ok {
		s.errors++
	}
	s.durationMsSum += durationMs
}

// countUnsent counts the items of a report frame that was handed to a writer and
// never written (the connection ended first) as DISCONNECTED.
func (r *reporter) countUnsent(frame []byte) {
	var msg InvocationsObservedMessage
	if json.Unmarshal(frame, &msg) != nil {
		return
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	for _, it := range msg.Items {
		r.dropLocked(it.FunctionID, dropDisconnected, 1)
	}
}

// written is called by the writer after it wrote a report frame.
func (r *reporter) written() {
	if r.deferred.CompareAndSwap(true, false) {
		r.kick()
	}
}

// discardFunction drops the buffered items of a function that cannot be confirmed
// (FUNCTION_NAMESPACE_CONFLICT), counted as INACTIVE (SDK-PARITY decision 22).
func (r *reporter) discardFunction(functionID string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	kept := r.items[:0]
	var dropped uint64
	r.bytes = 0
	for _, it := range r.items {
		if it.functionID == functionID {
			dropped++
			continue
		}
		kept = append(kept, it)
		r.bytes += len(it.json)
	}
	for i := len(kept); i < len(r.items); i++ {
		r.items[i] = nil
	}
	r.items = kept
	if dropped == 0 {
		return
	}
	r.dropLocked(functionID, dropInactive, dropped)
}

// flush sends what can be sent: items whose function the engine confirmed on the
// current connection, in frames under the frame limit, while the socket keeps up. The
// rest stays buffered.
func (r *reporter) flush() {
	epoch, online, v2 := r.host.reportOnline()
	r.mu.Lock()
	if r.lingerTimer != nil {
		r.lingerTimer.Stop()
		r.lingerTimer = nil
	}
	if len(r.items) == 0 || !online {
		r.mu.Unlock()
		return
	}
	// What the writer has not written yet counts against the bound: defer the flush
	// until it catches up, rather than piling frames up behind a slow socket. deferred
	// is set before the budget is read, so a written() in between still wakes a flush.
	r.deferred.Store(true)
	inFlight := r.host.reportQueued() + r.sending
	budget := r.limits.maxBytes - inFlight
	if budget <= 0 {
		r.mu.Unlock()
		return
	}
	overBudget := false
	statuses := map[string]reportStatus{}
	var ready, held []*reportItem
	gone := false
	for _, it := range r.items {
		fn := it.functionID
		st, ok := statuses[fn]
		if !ok {
			st = r.host.reportStatus(fn, epoch)
			statuses[fn] = st
		}
		switch st {
		case reportReady:
			// over the budget: next flush, unless nothing at all is in flight (an item
			// larger than what is left must still go once the socket is idle)
			if len(it.json) > budget && (len(ready) > 0 || inFlight > 0) {
				held = append(held, it)
				overBudget = true
				continue
			}
			budget -= len(it.json)
			ready = append(ready, it)
		case reportWait:
			held = append(held, it)
		default:
			// released or no longer registered here: its observers are not ours to serve
			gone = true
			r.dropLocked(fn, dropInactive, 1)
		}
	}
	if !overBudget {
		r.deferred.CompareAndSwap(true, false)
	}
	if len(ready) == 0 && !gone {
		r.mu.Unlock()
		return
	}
	r.items = held
	r.bytes = 0
	for _, it := range held {
		r.bytes += len(it.json)
	}
	// Held items wait for their function's functionregistered, which kicks a flush.
	type batch struct {
		data  []byte
		items []*reportItem
	}
	var batches []batch
	var cur batch
	for _, it := range ready {
		if it.v2 != v2 && it.render(v2) != nil {
			continue
		}
		if len(it.json)+reportFrameOverhead > r.limits.maxFrameBytes {
			r.dropLocked(it.functionID, dropTooLarge, 1)
			continue
		}
		if cur.data != nil && len(cur.data)+1+len(it.json)+len(reportFrameTail) > r.limits.maxFrameBytes {
			cur.data = append(cur.data, reportFrameTail...)
			batches = append(batches, cur)
			cur = batch{}
		}
		if cur.data == nil {
			cur.data = append([]byte(nil), reportFrameHead...)
		} else {
			cur.data = append(cur.data, ',')
		}
		cur.data = append(cur.data, it.json...)
		cur.items = append(cur.items, it)
	}
	if cur.data != nil {
		cur.data = append(cur.data, reportFrameTail...)
		batches = append(batches, cur)
	}
	for _, b := range batches {
		r.sending += len(b.data)
	}
	r.mu.Unlock()

	for i, b := range batches {
		sent := r.host.sendReport(epoch, b.data)
		r.mu.Lock()
		r.sending -= len(b.data)
		r.mu.Unlock()
		if sent {
			continue
		}
		for _, rest := range batches[i+1:] {
			r.mu.Lock()
			r.sending -= len(rest.data)
			r.mu.Unlock()
		}
		// The connection closed under us: the rest goes back to the buffer, in order,
		// and waits for the function's confirmation on the next connection.
		var back []*reportItem
		for _, rest := range batches[i:] {
			back = append(back, rest.items...)
		}
		r.putBack(back)
		return
	}
}

// putBack returns items whose frame could not be sent to the front of the buffer.
func (r *reporter) putBack(items []*reportItem) {
	r.mu.Lock()
	defer r.mu.Unlock()
	for _, it := range items {
		r.bytes += len(it.json)
	}
	r.items = append(items, r.items...)
}

// sendStats sends localinvocationstats when there is anything to report; it keeps
// counting while offline.
func (r *reporter) sendStats(now time.Time) bool {
	r.mu.Lock()
	if len(r.stats) == 0 && len(r.drops) == 0 {
		r.windowStart = now
		r.mu.Unlock()
		return false
	}
	r.mu.Unlock()
	epoch, online, _ := r.host.reportOnline()
	if !online {
		return false
	}

	r.mu.Lock()
	msg := &LocalInvocationStatsMessage{WindowMs: uint64(max(0, now.Sub(r.windowStart).Milliseconds()))}
	stats, statsOrder := r.stats, r.statsOrder
	drops, dropsOrder := r.drops, r.dropsOrder
	for _, fn := range statsOrder {
		s := stats[fn]
		msg.Functions = append(msg.Functions, LocalFunctionStats{FunctionID: fn, Count: s.count, Errors: s.errors, DurationMsSum: s.durationMsSum})
	}
	for _, k := range dropsOrder {
		msg.DroppedReports = append(msg.DroppedReports, DroppedReports{FunctionID: k.functionID, Reason: k.reason, Count: drops[k]})
	}
	r.stats, r.statsOrder = map[string]*functionStats{}, nil
	r.drops, r.dropsOrder = map[dropKey]uint64{}, nil
	start := r.windowStart
	r.windowStart = now
	r.mu.Unlock()

	frame, err := MarshalMessage(msg)
	if err == nil && r.host.sendReport(epoch, frame) {
		return true
	}
	// not sent: put the counts back for the next window
	r.mu.Lock()
	r.windowStart = start
	for _, fn := range statsOrder {
		s := stats[fn]
		cur := r.stats[fn]
		if cur == nil {
			r.stats[fn] = s
			r.statsOrder = append(r.statsOrder, fn)
			continue
		}
		cur.count += s.count
		cur.errors += s.errors
		cur.durationMsSum += s.durationMsSum
	}
	for _, k := range dropsOrder {
		if _, ok := r.drops[k]; !ok {
			r.dropsOrder = append(r.dropsOrder, k)
		}
		r.drops[k] += drops[k]
	}
	r.mu.Unlock()
	return false
}

// clear forgets everything buffered (the engine does not support observe).
func (r *reporter) clear() {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.items = nil
	r.bytes = 0
}
