package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"sync"
	"time"
)

// This file applies the stream rules to calls and invocations: the outbound walk that
// replaces placeholders with pipes (prepareStreams) and the pumps it starts, how a call
// waits for the sinks it passed, the guard that settles the streams an invocation
// received, in-process sinks, and the forms observers see. The rules are numbered as in
// the other SDKs: R2 output and R3 input of a handler, R4 a call waits for its sinks,
// R5 a stream sent on, R6 one reader, R9 no stream in an enqueued call.

var (
	channelIDKey    = []byte(`"channel_id"`)
	placeholderMark = []byte(`"local:`)
	writeMark       = []byte(`"write"`)
)

// noDepthLimit is the depth of the walks for placeholders, which must all be found:
// one left in a value would reach the wire.
const noDepthLimit = math.MaxInt

// sinkTimeout is the reason a sink still open at the call's timeout is aborted with.
const sinkTimeout = "the call timed out before its sinks ended"

// decodeValue decodes raw JSON with numbers kept as written.
func decodeValue(raw json.RawMessage) (any, bool) {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		return nil, false
	}
	return v, true
}

// encodeValue encodes a value decoded by decodeValue.
func encodeValue(v any) (json.RawMessage, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimSuffix(buf.Bytes(), []byte("\n")), nil
}

// refOf reads v as a stream ref: an object with a string channel_id, a string
// access_key and a direction "read" or "write".
func refOf(v any) (StreamChannelRef, bool) {
	m, ok := v.(map[string]any)
	if !ok {
		return StreamChannelRef{}, false
	}
	id, ok1 := m["channel_id"].(string)
	key, ok2 := m["access_key"].(string)
	dir, ok3 := m["direction"].(string)
	if !ok1 || !ok2 || !ok3 || (dir != string(ChannelRead) && dir != string(ChannelWrite)) {
		return StreamChannelRef{}, false
	}
	return StreamChannelRef{ChannelID: id, AccessKey: key, Direction: ChannelDirection(dir)}, true
}

// visitStreams calls visit with every stream in v, a ref or a placeholder, down to
// limit levels.
func visitStreams(v any, depth, limit int, visit func(StreamChannelRef)) {
	if depth > limit {
		return
	}
	switch t := v.(type) {
	case map[string]any:
		if ref, ok := refOf(t); ok {
			visit(ref)
			return
		}
		for _, item := range t {
			visitStreams(item, depth+1, limit, visit)
		}
	case []any:
		for _, item := range t {
			visitStreams(item, depth+1, limit, visit)
		}
	}
}

// visitPlaceholders calls visit with every placeholder in v.
func visitPlaceholders(v any, visit func(token string, dir ChannelDirection)) {
	visitStreams(v, 0, noDepthLimit, func(ref StreamChannelRef) {
		if token, ok := placeholderToken(ref); ok {
			visit(token, ref.Direction)
		}
	})
}

// visitRefs calls visit with every ref in v that is not a placeholder, to walkDepth.
func visitRefs(v any, visit func(StreamChannelRef)) {
	visitStreams(v, 0, walkDepth, func(ref StreamChannelRef) {
		if _, ok := placeholderToken(ref); !ok {
			visit(ref)
		}
	})
}

// replaceStreams replaces each stream in v with what with returns for it (v itself when
// with returns false), and returns v.
func replaceStreams(v any, with func(StreamChannelRef) (any, bool)) any {
	if ref, ok := refOf(v); ok {
		if out, ok := with(ref); ok {
			return out
		}
		return v
	}
	switch t := v.(type) {
	case map[string]any:
		for k, item := range t {
			t[k] = replaceStreams(item, with)
		}
	case []any:
		for i, item := range t {
			t[i] = replaceStreams(item, with)
		}
	}
	return v
}

// mayHavePlaceholder: a cheap test that raw cannot hold a placeholder.
func mayHavePlaceholder(raw json.RawMessage) bool {
	return placeholdersUsed.Load() && bytes.Contains(raw, placeholderMark)
}

// hasPlaceholder reports whether raw holds a placeholder: a stream that must not reach
// the wire as it is.
func hasPlaceholder(raw json.RawMessage) bool {
	if !mayHavePlaceholder(raw) {
		return false
	}
	v, ok := decodeValue(raw)
	if !ok {
		return false
	}
	found := false
	visitPlaceholders(v, func(string, ChannelDirection) { found = true })
	return found
}

// containsStream reports whether raw holds a stream: a ref or a placeholder.
func containsStream(raw json.RawMessage) bool {
	if !bytes.Contains(raw, channelIDKey) {
		return false
	}
	v, ok := decodeValue(raw)
	if !ok {
		return false
	}
	found := false
	visitStreams(v, 0, noDepthLimit, func(StreamChannelRef) { found = true })
	return found
}

// hasWriteStream reports whether raw holds a stream to write: a write ref, or a local
// one. A call whose result holds one does not wait for its sinks: its caller writes it.
func hasWriteStream(raw json.RawMessage) bool {
	if !bytes.Contains(raw, writeMark) || !bytes.Contains(raw, channelIDKey) {
		return false
	}
	v, ok := decodeValue(raw)
	if !ok {
		return false
	}
	found := false
	visitStreams(v, 0, walkDepth, func(ref StreamChannelRef) {
		found = found || ref.Direction == ChannelWrite
	})
	return found
}

// checkSentOnce refuses a payload that holds one stream twice, a stream already read to
// the end, or one already sent (STREAM_IN_USE), on every path, before anything is sent.
func checkSentOnce(raw json.RawMessage) error {
	if !placeholdersUsed.Load() || !bytes.Contains(raw, channelIDKey) {
		return nil
	}
	v, ok := decodeValue(raw)
	if !ok {
		return nil
	}
	seen := map[receivedKey]bool{}
	twice, ended, sent := false, false, false
	note := func(key receivedKey) {
		twice = twice || seen[key]
		seen[key] = true
	}
	visitStreams(v, 0, walkDepth, func(ref StreamChannelRef) {
		token, ok := placeholderToken(ref)
		if !ok {
			note(receivedKey{ref.ChannelID, ref.Direction})
			return
		}
		// A received stream sent again is a placeholder of its own ref.
		id := ref.ChannelID
		if e, ok := peekPlaceholder(token); ok {
			switch {
			case e.in != nil:
				ended = ended || e.in.readToTheEnd()
				sent = sent || e.in.sentOn()
				if e.in.ref != nil {
					id = e.in.ref.ChannelID
				}
			case e.out != nil && e.out.ref != nil:
				id = e.out.ref.ChannelID
			}
		}
		note(receivedKey{id, ref.Direction})
	})
	switch {
	case twice:
		return errSentTwice()
	case ended:
		return errReadToTheEnd()
	case sent:
		return errAlreadySent()
	}
	return nil
}

// release drops the streams raw carries as placeholders: a value nobody reads (a void
// call's result, a call refused before it was sent).
func release(raw json.RawMessage) {
	if !mayHavePlaceholder(raw) {
		return
	}
	v, ok := decodeValue(raw)
	if !ok {
		return
	}
	visitPlaceholders(v, func(token string, _ ChannelDirection) { claimPlaceholder(token) })
}

// markTransferred marks the received refs raw passes on as raw JSON (a call payload, a
// result) as sent: their guards leave them to the other side.
func markTransferred(raw json.RawMessage) {
	if receivedLen.Load() == 0 || !bytes.Contains(raw, channelIDKey) {
		return
	}
	v, ok := decodeValue(raw)
	if !ok {
		return
	}
	visitRefs(v, func(ref StreamChannelRef) {
		if e := lookupReceived(receivedKey{ref.ChannelID, ref.Direction}); e != nil {
			if e.in != nil {
				e.in.transfer()
			} else {
				e.out.transfer()
			}
		}
	})
}

func enqueueRejection() error {
	return streamError(CodeStreamNotAllowed, "a payload with a stream cannot be enqueued: streams need a live call "+
		"(the job can run after the pipe expired, and a retry cannot read the stream again); call it, or use VoidAction")
}

var errStreamInMetadata = errors.New("iii: a stream cannot travel in metadata: put it in the payload")

// checkCallStreams applies the checks every call makes before anything is sent: no
// stream in an enqueued call (R9), each stream once, no stream in the metadata. The
// received refs the payload passes on are the callee's then.
func checkCallStreams(req CallRequest) error {
	payload := req.payload()
	if req.Action != nil && req.Action.Type == "enqueue" && containsStream(payload) {
		release(payload)
		return enqueueRejection()
	}
	if err := checkSentOnce(payload); err != nil {
		release(payload)
		return err
	}
	if hasPlaceholder(req.Metadata) {
		release(payload)
		release(req.Metadata)
		return errStreamInMetadata
	}
	markTransferred(payload)
	return nil
}

// =====================================================================================
// Pumps
// =====================================================================================

// pump moves the frames of one stream: a source into the writer end of its pipe, or
// the reader end of a pipe into its sink.
type pump struct {
	cancel context.CancelCauseFunc
	out    *outputInner
	done   chan struct{}
	err    error
}

func startPump(src frameSource, out *outputInner) *pump {
	ctx, cancel := context.WithCancelCause(context.Background())
	p := &pump{cancel: cancel, out: out, done: make(chan struct{})}
	go func() {
		defer close(p.done)
		defer cancel(nil)
		p.err = runPump(ctx, src, out)
	}()
	return p
}

// stop aborts the pump's stream with reason, at once: the stream it writes has ended
// when stop returns, without waiting for a write in flight (a sink whose Write blocks:
// the pump's goroutine ends when that Write returns).
func (p *pump) stop(reason string) {
	p.cancel(errors.New(reason))
	p.out.abortNow(reason)
}

// runPump moves src into out until src ends (then out closes) or fails (then out
// aborts with its reason). A cancel aborts out with the cancel's reason; a failed write
// releases src.
func runPump(ctx context.Context, src frameSource, out *outputInner) error {
	stopSrc := context.AfterFunc(ctx, src.cancel)
	defer stopSrc()
	stopped := func() error {
		reason := context.Cause(ctx).Error()
		_ = out.abort(reason)
		return fmt.Errorf("iii: stream stopped: %s", reason)
	}
	for {
		frame, err := src.next(ctx)
		if ctx.Err() != nil {
			return stopped()
		}
		switch {
		case err == io.EOF:
			return out.close()
		case err != nil:
			_ = out.abort(abortReason(err))
			return err
		}
		if frame.Type == FrameText {
			err = out.sendText(ctx, frame.Data)
		} else {
			err = out.write(ctx, frame.Data)
		}
		if err != nil {
			src.cancel()
			if ctx.Err() != nil {
				return stopped()
			}
			return err
		}
	}
}

// abortReason is the reason an error aborts a stream with: an abort's own reason, else
// its message.
func abortReason(err error) string {
	var ie *InvocationError
	if errors.As(err, &ie) && ie.Code == CodeStreamAborted {
		return ie.Message
	}
	return err.Error()
}

// pumps are the pumps started for what one call or result sends.
type pumps struct {
	sources []*pump
	sinks   []*pump
}

// settle settles a call that sent these pumps' streams (R4). With a result it returns
// once every sink got its whole stream, and fails with the error of a sink that did
// not (STREAM_ABORTED for an abort). With an error it stops every pump, aborting their
// streams with it, first.
//
// detach (a void call) leaves the pumps running on their own once the call was sent,
// and so does a result that holds a stream to write (the caller writes it after the
// call returned, maybe into one of these sinks). The wait ends at deadline, the call's
// timeout, or when ctx ends: the sinks still open are then aborted, and the call fails
// with ErrTimeout (or ctx's error). Stopping a pump does not wait for it: a sink whose
// Write blocks does not hold the call past its deadline. A sink that failed (its
// writer's error) is what the call fails with, whatever the function returned, unless
// ctx ended.
func (p *pumps) settle(ctx context.Context, result json.RawMessage, err error, detach bool, deadline time.Time) (json.RawMessage, error) {
	if p == nil || len(p.sources)+len(p.sinks) == 0 {
		return result, err
	}
	if err == nil && (detach || hasWriteStream(result)) {
		return result, nil
	}
	if err != nil {
		// The sinks are aborted before the call fails: a write to one after it fails.
		reason := err.Error()
		for _, pm := range p.sources {
			pm.stop(reason)
		}
		for _, pm := range p.sinks {
			pm.stop(reason)
		}
		return nil, p.failure(ctx, err)
	}
	// Sources keep pumping: the function reads them.
	timer := time.NewTimer(time.Until(deadline))
	defer timer.Stop()
	stopSinks := func(reason string) {
		for _, pm := range p.sinks {
			pm.stop(reason)
		}
	}
	var failure error
	for _, pm := range p.sinks {
		select {
		case <-pm.done:
			if pm.err != nil && failure == nil {
				failure = pm.err
			}
		case <-timer.C:
			stopSinks(sinkTimeout)
			return nil, p.failure(ctx, ErrTimeout)
		case <-ctx.Done():
			stopSinks(ctx.Err().Error())
			return nil, ctx.Err()
		}
	}
	if failure != nil {
		return nil, p.failure(ctx, failure)
	}
	return result, nil
}

// failure is what a call that failed with err fails with: a sink it passed that failed
// is why (the function's writes failed after it), unless the caller's ctx ended.
func (p *pumps) failure(ctx context.Context, err error) error {
	if ctx.Err() != nil {
		return err
	}
	sinks := make([]*outputInner, len(p.sinks))
	for i, pm := range p.sinks {
		sinks[i] = pm.out
	}
	if failed := failedSink(sinks); failed != nil {
		return failed
	}
	return err
}

// =====================================================================================
// Outbound walk: placeholders become pipes
// =====================================================================================

type claimedStream struct {
	token string
	entry localStream
	src   frameSource
}

// abandon ends what a failed walk claimed: its sinks abort with err, its sources are
// released.
func abandon(claimed []claimedStream, err error) {
	reason := err.Error()
	for _, cl := range claimed {
		switch {
		case cl.entry.out != nil:
			_ = cl.entry.out.abort(reason)
		case cl.src != nil:
			cl.src.cancel()
		}
	}
}

// prepareStreams replaces the placeholders in raw (what a call or a result sends) with
// pipe refs and starts their pumps: each source pumped into the writer end of its pipe,
// the reader end of each pipe into its sink. The pipes are created in parallel; a
// failure fails before anything is sent. raw is returned as it is when it holds no
// placeholder.
func (c *Client) prepareStreams(ctx context.Context, raw json.RawMessage) (json.RawMessage, *pumps, error) {
	if !mayHavePlaceholder(raw) {
		return raw, nil, nil
	}
	v, ok := decodeValue(raw)
	if !ok {
		return raw, nil, nil
	}
	type found struct {
		token string
		dir   ChannelDirection
	}
	var tokens []found
	visitPlaceholders(v, func(token string, dir ChannelDirection) { tokens = append(tokens, found{token, dir}) })
	if len(tokens) == 0 {
		return raw, nil, nil
	}

	// Claim them all, or none.
	seen := make(map[string]bool, len(tokens))
	claimed := make([]claimedStream, 0, len(tokens))
	var failure error
	for _, t := range tokens {
		if seen[t.token] {
			failure = errSentTwice()
			break
		}
		seen[t.token] = true
		e, ok := claimPlaceholder(t.token)
		if ok && ((t.dir == ChannelRead && e.in != nil) || (t.dir == ChannelWrite && e.out != nil)) {
			claimed = append(claimed, claimedStream{token: t.token, entry: e})
			continue
		}
		if ok {
			registerPlaceholder(t.token, e)
		}
		failure = gone(placeholderPrefix + t.token)
		break
	}
	if failure != nil {
		for _, cl := range claimed {
			registerPlaceholder(cl.token, cl.entry)
		}
		return raw, nil, failure
	}

	for i := range claimed {
		var err error
		if in := claimed[i].entry.in; in != nil {
			claimed[i].src, err = in.takeSource()
		} else {
			err = claimed[i].entry.out.takeSink()
		}
		if err != nil {
			abandon(claimed[:i], err)
			return raw, nil, err
		}
	}

	type pipeRefs struct {
		writer, reader StreamChannelRef
		err            error
	}
	pipes := make([]pipeRefs, len(claimed))
	var wg sync.WaitGroup
	for i := range claimed {
		wg.Add(1)
		go func() {
			defer wg.Done()
			pipes[i].writer, pipes[i].reader, pipes[i].err = c.createPipe(ctx)
		}()
	}
	wg.Wait()
	for _, p := range pipes {
		if p.err != nil {
			abandon(claimed, p.err)
			return raw, nil, p.err
		}
	}

	ep := c.streamEndpoint()
	refs := make(map[string]StreamChannelRef, len(claimed))
	started := &pumps{}
	for i, cl := range claimed {
		if cl.entry.in != nil {
			refs[cl.token] = pipes[i].reader
			started.sources = append(started.sources, startPump(cl.src, newOutput(&pipes[i].writer, nil, ep)))
		} else {
			refs[cl.token] = pipes[i].writer
			started.sinks = append(started.sinks, startPump(&wsSource{ep: ep, ref: pipes[i].reader}, cl.entry.out))
		}
	}
	v = replaceStreams(v, func(ref StreamChannelRef) (any, bool) {
		token, ok := placeholderToken(ref)
		if !ok {
			return nil, false
		}
		wire, ok := refs[token]
		return wire, ok
	})
	out, err := encodeValue(v)
	if err != nil {
		_, err = started.settle(ctx, nil, err, false, time.Now())
		return raw, nil, err
	}
	return out, started, nil
}

// =====================================================================================
// In-process calls (R4 for local sinks)
// =====================================================================================

// handOverLocal hands the streams payload passes as placeholders to the handler of an
// in-process call, as pipes would carry them: the rest of each source moves to a new
// stream the handler reads, and the caller's counts as sent (reading or sending it again
// fails with STREAM_IN_USE). It returns the handler's sources, which a failed call
// aborts. As on the remote path (prepareStreams), a placeholder of no stream of this
// process fails the call before anything moves, and a source that cannot be sent (a
// read in flight, cancelled) fails it after releasing the sources already moved.
func handOverLocal(payload json.RawMessage) ([]*inputInner, error) {
	if !mayHavePlaceholder(payload) {
		return nil, nil
	}
	v, ok := decodeValue(payload)
	if !ok {
		return nil, nil
	}
	type source struct {
		token string
		in    *inputInner
	}
	var found []source
	var failure error
	visitPlaceholders(v, func(token string, dir ChannelDirection) {
		if failure != nil {
			return
		}
		e, ok := peekPlaceholder(token)
		switch {
		case ok && dir == ChannelRead && e.in != nil:
			found = append(found, source{token, e.in})
		case ok && dir == ChannelWrite && e.out != nil:
			// a sink: settleLocalSinks
		default:
			failure = gone(placeholderPrefix + token)
		}
	})
	if failure != nil {
		return nil, failure
	}
	moved := make([]*inputInner, 0, len(found))
	for _, f := range found {
		recv, err := f.in.moveOut()
		if err != nil {
			for _, in := range moved {
				in.end(readFailed, err)
			}
			return nil, err
		}
		registerPlaceholder(f.token, localStream{in: recv})
		moved = append(moved, recv)
	}
	return moved, nil
}

// abortLocalSources ends the sources of an in-process call that failed (its timeout,
// its ctx, a handler error), as a remote call stops its source pumps: a read of one in
// flight or later fails with STREAM_ABORTED, and the caller's reader is released.
func abortLocalSources(sources []*inputInner, err error) {
	reason := err.Error()
	for _, in := range sources {
		in.end(readFailed, streamAbortedError(reason))
	}
}

type localSink struct {
	token string
	out   *outputInner
}

// localSinks are the sinks payload passes as placeholders, for a call this worker
// dispatches in-process.
func localSinks(payload json.RawMessage) []localSink {
	if !mayHavePlaceholder(payload) {
		return nil
	}
	v, ok := decodeValue(payload)
	if !ok {
		return nil
	}
	var sinks []localSink
	visitPlaceholders(v, func(token string, dir ChannelDirection) {
		if dir != ChannelWrite {
			return
		}
		if e, ok := peekPlaceholder(token); ok && e.out != nil {
			sinks = append(sinks, localSink{token: token, out: e.out})
		}
	})
	return sinks
}

// settleLocalSinks settles an in-process call's sinks (R4), as pumps.settle does a
// remote call's: with a result, a sink its handler never took is closed (nothing else
// can end it), then the call waits for every sink to end, until deadline, and fails with
// STREAM_ABORTED if one aborted. A result that holds a stream to write is not waited
// for. With an error, the sinks still open are aborted with it. A sink that failed (its
// writer's error) is what the call fails with, whatever the handler returned, unless
// ctx ended.
func settleLocalSinks(ctx context.Context, sinks []localSink, result json.RawMessage, err error, deadline time.Time) (json.RawMessage, error) {
	if len(sinks) == 0 {
		return result, err
	}
	abortAll := func(reason string) {
		for _, s := range sinks {
			claimPlaceholder(s.token)
			_ = s.out.abort(reason)
		}
	}
	// A sink that failed is why the call failed (the handler's writes failed after it),
	// unless the caller's ctx ended.
	failure := func(err error) error {
		if ctx.Err() != nil {
			return err
		}
		outs := make([]*outputInner, len(sinks))
		for i, s := range sinks {
			outs[i] = s.out
		}
		if failed := failedSink(outs); failed != nil {
			return failed
		}
		return err
	}
	if err != nil {
		abortAll(err.Error())
		return nil, failure(err)
	}
	for _, s := range sinks {
		if _, ok := claimPlaceholder(s.token); ok {
			_ = s.out.close()
		}
	}
	if hasWriteStream(result) {
		return result, nil
	}
	timer := time.NewTimer(time.Until(deadline))
	defer timer.Stop()
	for _, s := range sinks {
		select {
		case <-s.out.done:
			if _, aborted, reason := s.out.outcome(); aborted {
				return nil, failure(streamAbortedError(reason))
			}
		case <-timer.C:
			abortAll(sinkTimeout)
			return nil, failure(ErrTimeout)
		case <-ctx.Done():
			abortAll(ctx.Err().Error())
			return nil, ctx.Err()
		}
	}
	return result, nil
}

// =====================================================================================
// The handler guard (R2, R3)
// =====================================================================================

// streamGuard holds the streams an invocation received, and settles them when its
// handler returns.
type streamGuard struct {
	id uint64
	// activation: the invocation is an observer activation.
	activation bool

	mu      sync.Mutex
	settled bool
	inputs  []*inputInner
	outputs []*outputInner
	keys    []receivedKey
}

func newStreamGuard(activation bool) *streamGuard {
	return &streamGuard{id: nextStreamID(), activation: activation}
}

// adopt makes e one of the guard's streams, registered under key while the handler
// runs.
func (g *streamGuard) adopt(key receivedKey, e *receivedEntry) {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.settled {
		return
	}
	e.guard = g
	if e.in != nil {
		e.in.handOver(g.id)
		g.inputs = append(g.inputs, e.in)
	} else {
		g.outputs = append(g.outputs, e.out)
	}
	g.keys = append(g.keys, key)
	storeReceived(key, e)
}

// trackInput tracks every stream in the handler's input as it arrived, so the guard
// applies to them whether the handler unmarshals them or not: a ref opens a new stream
// bound to c; a placeholder (a call this worker dispatched in-process) is the caller's
// stream itself, with no pipe. Unmarshalling a ref of the input gives the tracked stream.
func (g *streamGuard) trackInput(c *Client, raw json.RawMessage) {
	if !bytes.Contains(raw, channelIDKey) {
		return
	}
	v, ok := decodeValue(raw)
	if !ok {
		return
	}
	var ep *endpoint
	visitStreams(v, 0, walkDepth, func(ref StreamChannelRef) {
		key := receivedKey{ref.ChannelID, ref.Direction}
		if token, ok := placeholderToken(ref); ok {
			e, ok := peekPlaceholder(token)
			switch {
			case !ok:
			case ref.Direction == ChannelRead && e.in != nil:
				g.adopt(key, &receivedEntry{in: e.in})
			case ref.Direction == ChannelWrite && e.out != nil:
				g.adopt(key, &receivedEntry{out: e.out})
			}
			return
		}
		if ep == nil {
			ep = c.streamEndpoint()
		}
		if ref.Direction == ChannelRead {
			g.adopt(key, &receivedEntry{in: newInput(&ref, nil, ep)})
		} else {
			g.adopt(key, &receivedEntry{out: newOutput(&ref, nil, ep)})
		}
	})
}

// settle applies R2 and R3 once the handler returned out: it cancels input the handler
// started reading and did not finish, and starts closing or aborting output, without
// waiting for it.
func (g *streamGuard) settle(out outcome) {
	g.mu.Lock()
	g.settled = true
	inputs, outputs, keys := g.inputs, g.outputs, g.keys
	g.inputs, g.outputs, g.keys = nil, nil, nil
	g.mu.Unlock()
	dropReceived(keys, g)
	for _, in := range inputs {
		in.settle(g.id)
	}
	if len(outputs) == 0 {
		return
	}
	failed := out.errBody != nil
	reason := ""
	switch {
	case out.err != nil:
		reason = out.err.Error()
	case failed:
		reason = newInvocationError(out.errBody, "").Error()
	}
	resultNull := !failed && bytes.Equal(bytes.TrimSpace(out.result), []byte("null"))
	for _, o := range outputs {
		if end := o.settle(failed, reason, resultNull, g.activation); end != nil {
			go end()
		}
	}
}

// streamCall says how an invocation's streams are handled.
type streamCall struct {
	// answered: someone reads the result (not a void call); its streams are sent.
	answered bool
	// activation: the invocation is an observer activation.
	activation bool
	// observed: the input is an Observed envelope, its refs rewritten as StreamInfo:
	// nothing in it is the handler's.
	observed bool
}

// sendOutcome makes the streams in out's result ready to send: pipes replace the
// placeholders, and their pumps start (prepareStreams), when the result is answered;
// otherwise nobody reads them, and they are released. The received refs the result
// passes on are the caller's then.
func (c *Client) sendOutcome(out outcome, answered bool) outcome {
	if out.errBody != nil {
		return out
	}
	if !answered {
		release(out.result)
		return out
	}
	result, _, err := c.prepareStreams(c.lifeCtx, out.result)
	if err != nil {
		return outcome{errBody: errorBodyFromHandlerError(err), err: err}
	}
	out.result = result
	markTransferred(result)
	return out
}

// =====================================================================================
// Call results
// =====================================================================================

// trackResult tracks the refs in a call's result for streamTTL, so unmarshalling one
// gives a stream bound to c.
func (c *Client) trackResult(raw json.RawMessage) {
	if !bytes.Contains(raw, channelIDKey) {
		return
	}
	v, ok := decodeValue(raw)
	if !ok {
		return
	}
	var ep *endpoint
	visitRefs(v, func(ref StreamChannelRef) {
		if ep == nil {
			ep = c.streamEndpoint()
		}
		key := receivedKey{ref.ChannelID, ref.Direction}
		if ref.Direction == ChannelRead {
			storeReceived(key, &receivedEntry{in: newInput(&ref, nil, ep)})
		} else {
			storeReceived(key, &receivedEntry{out: newOutput(&ref, nil, ep)})
		}
	})
}

// =====================================================================================
// Observed calls: refs become StreamInfo
// =====================================================================================

func streamInfoValue(id string, dir ChannelDirection) map[string]any {
	return map[string]any{"stream": true, "direction": string(dir), "id": id}
}

// rewriteObserved rewrites an observer activation of an engine-built [Observed]
// envelope (its activation_id and observer_id are the invocation's): the stream refs
// under its input and output become [StreamInfo]. A call through an observer gets its
// activation id from the engine after the caller built the payload, so its payload
// never matches.
func rewriteObserved(raw json.RawMessage, activation *Activation) (json.RawMessage, bool) {
	if activation == nil || activation.ActivationID == "" {
		return raw, false
	}
	v, ok := decodeValue(raw)
	if !ok {
		return raw, false
	}
	envelope, ok := v.(map[string]any)
	if !ok || envelope["activation_id"] != activation.ActivationID || envelope["observer_id"] != activation.ObserverID {
		return raw, false
	}
	changed := false
	for _, key := range []string{"input", "output"} {
		if field, ok := envelope[key]; ok {
			envelope[key] = replaceStreams(field, func(ref StreamChannelRef) (any, bool) {
				changed = true
				return streamInfoValue(ref.ChannelID, ref.Direction), true
			})
		}
	}
	if !changed {
		return raw, true
	}
	out, err := encodeValue(envelope)
	if err != nil {
		return raw, true
	}
	return out, true
}

// reportForm is raw as a report of an in-process call carries it: a local stream
// becomes a [StreamInfo], since its placeholder must not reach the wire.
func reportForm(raw json.RawMessage) json.RawMessage {
	if !hasPlaceholder(raw) {
		return raw
	}
	v, _ := decodeValue(raw)
	v = replaceStreams(v, func(ref StreamChannelRef) (any, bool) {
		if _, ok := placeholderToken(ref); !ok {
			return nil, false
		}
		return streamInfoValue(ref.ChannelID, ref.Direction), true
	})
	out, err := encodeValue(v)
	if err != nil {
		return raw
	}
	return out
}
