package iii

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"slices"
	"sync"
	"sync/atomic"
	"time"

	"github.com/iii-hq/iii/sdk/packages/go/iii/filter"
)

// This file is the hosting side of observe: per-function observer state as the engine
// pushed it (functionregistered, observeradded, observerremoved), the added and removed
// hooks at both levels run on the function's hook queue, and the Observer handed to the
// hooks.

// observeHookTimeout is how long an added hook may run before the observer is
// rejected with OWNER_TIMEOUT.
const observeHookTimeout = 30 * time.Second

// defaultCapture is what an absent capture means.
var defaultCapture = []Capture{CaptureInput, CaptureOutput, CaptureCaller}

// errObserverRemoved is what Accepted returns for an observer removed before the SDK
// accepted it.
var errObserverRemoved = errors.New("iii: observer removed before it was accepted")

// errAcceptedInAddedHook is what Accepted returns when it is called with the ctx of the
// observer's own added hook, which the ack waits for.
var errAcceptedInAddedHook = errors.New("iii: Accepted called with the ctx of the observer's own OnObserverAdded, " +
	"which the ack waits for; wait from a goroutine the hook starts, with a ctx of its own")

// addingKey marks the ctx of an added hook with the observer it runs for.
type addingKey struct{}

// functionObservers is one registered function's observers. Its hooks run one at a
// time, in revision order, on its id's hook queue (Client.enqueueHook). It never calls
// into the client or the reporter while holding mu.
type functionObservers struct {
	client              *Client
	functionID          string
	localDispatchOption bool
	// private: calls to it pass values by reference.
	private bool

	// state is what a call reads when it starts: whether it runs in-process, the active
	// observers and the hooks that deliver to them. Swapped under mu, loaded once per
	// call.
	state atomic.Pointer[callState]
	// standingBy: a standby registration is waiting for the function's host to leave.
	standingBy atomic.Bool
	// matchPanicLogged: a panic of the match hook was logged.
	matchPanicLogged atomic.Bool

	mu sync.Mutex
	// confirmedEpoch is the connection functionregistered arrived on, or -1.
	confirmedEpoch int64
	// syncedEpoch is the connection whose functionregistered set was applied and
	// acked, or -1: reports wait for it, so none reaches the engine before the acks
	// that make the replayed observers active again.
	syncedEpoch int64
	// releasedEpoch is the connection on which the engine refused the function, or -1.
	// On that connection its reports are gone (INACTIVE), those of a call already
	// running included; a later registration may be accepted, and functionregistered
	// clears it.
	releasedEpoch int64
	// unsupportedEpoch is the connection on which the function was left unregistered
	// because it needs observe.v2 and the engine lacks it, or -1.
	unsupportedEpoch int64
	namespace        string
	// revision is the highest revision received on this connection.
	revision int64
	known    map[string]*Observer
	disposed bool
	// releases counts the releases (refusals, withdrawals, unregister): a message
	// handled after one that followed it is not applied.
	releases int
	// wantMatch is the match hook last set; the hook queue installs it (applyMatch).
	wantMatch *MatchHook
	// nextSeq is the seq of the observer activated last (Observer.seq).
	nextSeq uint64
}

func newFunctionObservers(c *Client, functionID string, localDispatch, private bool) *functionObservers {
	f := &functionObservers{
		client:              c,
		functionID:          functionID,
		localDispatchOption: localDispatch,
		private:             private,
		confirmedEpoch:      -1,
		syncedEpoch:         -1,
		releasedEpoch:       -1,
		unsupportedEpoch:    -1,
		revision:            -1,
		known:               map[string]*Observer{},
	}
	f.state.Store(&callState{})
	return f
}

// swapLocked replaces the call state with a copy that change edited. The caller holds
// mu.
func (f *functionObservers) swapLocked(change func(st *callState)) {
	st := *f.state.Load()
	change(&st)
	f.state.Store(&st)
}

// enqueue runs task on the function id's hook queue.
func (f *functionObservers) enqueue(task func()) { f.client.enqueueHook(f.functionID, task) }

// compiledSet is the active observers.
func (f *functionObservers) compiledSet() []*Observer { return f.state.Load().observers }

func (f *functionObservers) isObserved() bool { return len(f.compiledSet()) > 0 }

// isObservedFor: on a function matched by keys, whether an observer is filed under one
// of the call's keys; otherwise whether any observer is active.
func (f *functionObservers) isObservedFor(call *Call) bool {
	st := f.state.Load()
	if len(st.observers) == 0 {
		return false
	}
	if call == nil || st.index == nil {
		return true
	}
	return st.index.has(f.callKeys(st.match, call))
}

func (f *functionObservers) observers() []*Observer { return slices.Clone(f.compiledSet()) }

// installHooks sets the hooks of a function being registered, before its registration
// is sent: no observer is active yet, so a match hook by keys starts with an empty
// index.
func (f *functionObservers) installHooks(h *observerHooks) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.wantMatch = h.match
	f.swapLocked(func(st *callState) { st.deliver, st.match, st.index = h.deliver, h.match, emptyIndexFor(h.match) })
}

// emptyIndexFor is the index a match hook starts with: empty for a hook by keys, nil
// otherwise.
func emptyIndexFor(m *MatchHook) *keyIndex {
	if m.byKeys() {
		return &keyIndex{}
	}
	return nil
}

// setDeliver installs the function's deliver hook: calls that start after it use it.
func (f *functionObservers) setDeliver(hook DeliverHook) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.swapLocked(func(st *callState) { st.deliver = hook })
}

// heldOn: the function stays unregistered on connection epoch (it needs observe.v2, and
// the engine lacks it), withdrawn there if its registration had gone out.
func (f *functionObservers) heldOn(epoch uint64) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.unsupportedEpoch == int64(epoch)
}

// syncedOn: functionregistered arrived on connection epoch and its set was applied
// and acked.
func (f *functionObservers) syncedOn(epoch uint64) bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.syncedEpoch == int64(epoch)
}

// reportStatus is what a flush on connection epoch does with this function's
// reports: send them, hold them, or drop them as INACTIVE (the engine refused the
// function on that connection, or it stays unregistered there for lack of observe.v2).
func (f *functionObservers) reportStatus(epoch uint64) reportStatus {
	f.mu.Lock()
	defer f.mu.Unlock()
	switch {
	case f.releasedEpoch == int64(epoch), f.unsupportedEpoch == int64(epoch):
		return reportGone
	case f.syncedEpoch == int64(epoch):
		return reportReady
	}
	return reportWait
}

// onFunctionRegistered is always accepted and replaces the set. It runs on the read
// goroutine; the diff and the hooks run on the function's queue.
func (f *functionObservers) onFunctionRegistered(msg *FunctionRegisteredMessage, epoch uint64) {
	f.mu.Lock()
	if f.disposed {
		f.mu.Unlock()
		return
	}
	f.confirmedEpoch = int64(epoch)
	f.releasedEpoch = -1
	f.namespace = msg.Namespace
	f.revision = int64(msg.Revision)
	f.swapLocked(func(st *callState) {
		st.local, st.namespace = msg.LocalDispatch && f.localDispatchOption, msg.Namespace
	})
	releases := f.releases
	f.mu.Unlock()
	f.standingBy.Store(false)
	list := slices.Clone(msg.Observers)
	f.enqueue(func() { f.applySet(list, msg.Revision, msg.Namespace, epoch, releases) })
}

func (f *functionObservers) onObserverAdded(msg *ObserverAddedMessage, epoch uint64) {
	f.mu.Lock()
	if f.disposed || int64(msg.Revision) <= f.revision {
		f.mu.Unlock()
		return
	}
	f.revision = int64(msg.Revision)
	releases := f.releases
	f.mu.Unlock()
	info := msg.Observer
	f.enqueue(func() {
		f.mu.Lock()
		released, held := f.releasedSinceLocked(releases), f.known[info.ID]
		f.mu.Unlock()
		if released {
			// announced before the function was released (refused, withdrawn,
			// unregistered): it is not this worker's, so no hook runs and no ack goes out
			return
		}
		if held != nil {
			if f.reack(held, info, msg.Revision, msg.Namespace, epoch, releases, false) {
				return
			}
			// other args, or rejected late: a fresh observer, after the one held is removed
			f.remove(info.ID)
		}
		f.add(info, msg.Revision, msg.Namespace, epoch, releases)
	})
}

func (f *functionObservers) onObserverRemoved(msg *ObserverRemovedMessage) {
	f.mu.Lock()
	if f.disposed || int64(msg.Revision) <= f.revision {
		f.mu.Unlock()
		return
	}
	f.revision = int64(msg.Revision)
	f.mu.Unlock()
	id := msg.ID
	f.enqueue(func() { f.remove(id) })
}

// onRegistrationRejected (the engine refused the registration on connection epoch): no
// local dispatch, no observers; every held observer goes through the removed hooks,
// and the function's reports are gone on that connection.
func (f *functionObservers) onRegistrationRejected(epoch uint64) {
	f.mu.Lock()
	f.releasedEpoch = int64(epoch)
	f.mu.Unlock()
	f.release()
}

// release drops the confirmation, local dispatch and the observer set. The observers
// are removed on the function's queue, after the adds queued before: an add still
// queued or running when the release happens sees it and undoes its own hooks (add).
func (f *functionObservers) release() {
	f.mu.Lock()
	f.releases++
	f.confirmedEpoch = -1
	f.syncedEpoch = -1
	f.swapLocked(func(st *callState) {
		for _, obs := range st.observers {
			obs.active.Store(false)
		}
		st.local, st.namespace, st.observers, st.index, st.filters = false, "", nil, emptyIndexFor(st.match), nil
	})
	f.mu.Unlock()
	f.enqueue(func() {
		f.mu.Lock()
		ids := make([]string, 0, len(f.known))
		for id := range f.known {
			ids = append(ids, id)
		}
		f.mu.Unlock()
		slices.Sort(ids)
		for _, id := range ids {
			f.remove(id)
		}
	})
}

// releasedLocked: replaced, or refused / not confirmed by the engine. Its observers
// are not to be activated or acked.
func (f *functionObservers) releasedLocked() bool {
	return f.disposed || f.confirmedEpoch < 0
}

// releasedSinceLocked: released now, or since the release count was releases (when the
// message being applied arrived), even if a later registration was confirmed since.
// The caller holds mu.
func (f *functionObservers) releasedSinceLocked(releases int) bool {
	return f.releasedLocked() || f.releases != releases
}

// dispose: the function was unregistered (a later registration of the same id has
// observe state of its own).
func (f *functionObservers) dispose() {
	f.mu.Lock()
	f.disposed = true
	f.mu.Unlock()
	f.release()
}

func (f *functionObservers) isDisposed() bool {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.disposed
}

// hookQueue is the observer hooks of one function id waiting to run.
type hookQueue struct{ tasks []func() }

// enqueueHook runs task after the tasks queued before it for functionID, on a goroutine
// of that id's own: the hooks of a function run one at a time, across its registrations
// too, and those of different functions concurrently. An id has a queue while it has
// tasks.
func (c *Client) enqueueHook(functionID string, task func()) {
	c.hookQueueMu.Lock()
	defer c.hookQueueMu.Unlock()
	if q := c.hookQueues[functionID]; q != nil {
		q.tasks = append(q.tasks, task)
		return
	}
	q := &hookQueue{tasks: []func(){task}}
	c.hookQueues[functionID] = q
	go c.drainHooks(functionID, q)
}

func (c *Client) drainHooks(functionID string, q *hookQueue) {
	for {
		c.hookQueueMu.Lock()
		if len(q.tasks) == 0 {
			delete(c.hookQueues, functionID)
			c.hookQueueMu.Unlock()
			return
		}
		task := q.tasks[0]
		q.tasks[0] = nil
		q.tasks = q.tasks[1:]
		c.hookQueueMu.Unlock()
		func() {
			defer func() {
				if r := recover(); r != nil {
					log.Printf("iii: observe hook failed for %q: %v", functionID, r)
				}
			}()
			task()
		}()
	}
}

// activateLocked puts obs in the active set (before its ack is sent) and, on a function
// matched by keys, files it under obs.keys. The caller holds mu.
func (f *functionObservers) activateLocked(obs *Observer) {
	if obs.active.Load() {
		return
	}
	obs.active.Store(true)
	f.nextSeq++
	obs.seq = f.nextSeq
	f.swapLocked(func(st *callState) {
		st.observers = append(slices.Clip(st.observers), obs)
		if st.index != nil {
			st.index = st.index.with(obs, obs.keys)
		}
		st.filters = f.fileLocked(st.filters, obs)
	})
}

// fileLocked is filters with obs filed at its seq, and records the condition it is
// filed under. The caller holds mu.
func (f *functionObservers) fileLocked(filters *filterIndex, obs *Observer) *filterIndex {
	next, cond := filters.with(obs, obs.seq)
	obs.cond.Store(cond)
	return next
}

// deactivateLocked takes obs out of the active set, the key index and the filter index.
// The caller holds mu.
func (f *functionObservers) deactivateLocked(obs *Observer) {
	if !obs.active.Load() {
		return
	}
	obs.active.Store(false)
	f.swapLocked(func(st *callState) {
		st.observers = slices.DeleteFunc(slices.Clone(st.observers), func(o *Observer) bool { return o == obs })
		if st.index != nil {
			st.index = st.index.without(obs, obs.keys)
		}
		st.filters = st.filters.without(obs, obs.seq, obs.cond.Load())
	})
}

func (f *functionObservers) ack(id string, revision uint64, namespace string, epoch uint64, errBody *ErrorBody) {
	frame, err := prepareJSON(&ObserverAckMessage{FunctionID: f.functionID, Namespace: namespace, ID: id, Revision: revision, Error: errBody})
	if err != nil {
		log.Printf("iii: observerack for %q not encodable: %v", id, err)
		return
	}
	f.client.sendOnEpoch(epoch, frame)
}

// applySet diffs the engine's full set against what this SDK holds: removed ->
// removed hooks; new -> added hooks; held -> acked again without the hooks (reack; the
// engine marks a replayed set pending until acked). A held observer with other args is
// removed, then added. Nothing is applied once the function was released after the
// set arrived (releases is the release count then).
func (f *functionObservers) applySet(list []ObserverInfo, revision uint64, namespace string, epoch uint64, releases int) {
	f.mu.Lock()
	released := f.releasedSinceLocked(releases)
	f.mu.Unlock()
	if released {
		return // the function is not this worker's
	}
	incoming := make(map[string]bool, len(list))
	for _, info := range list {
		incoming[info.ID] = true
	}
	f.mu.Lock()
	var gone []string
	for id := range f.known {
		if !incoming[id] {
			gone = append(gone, id)
		}
	}
	f.mu.Unlock()
	slices.Sort(gone)
	for _, id := range gone {
		f.remove(id)
	}
	for _, info := range list {
		f.mu.Lock()
		held := f.known[info.ID]
		f.mu.Unlock()
		if held != nil {
			if f.reack(held, info, revision, namespace, epoch, releases, true) {
				continue
			}
			f.remove(info.ID)
		}
		f.add(info, revision, namespace, epoch, releases)
	}
	f.mu.Lock()
	synced := f.confirmedEpoch == int64(epoch)
	if synced {
		f.syncedEpoch = int64(epoch)
	}
	f.mu.Unlock()
	if synced {
		// reports held for this function can go now, after the acks
		f.client.reporter.kick()
	}
}

// add runs the added hooks of both levels, the worker's then the function's, computes
// the observer's match keys when the function is matched by keys, then activates the
// observer and acks it. A hook that fails, or keys that panic, reject the observer, and
// the removed hook of each level whose added hook resolved runs. Nothing runs for a
// function released (refused, withdrawn, unregistered) since the message that announced
// the observer arrived (releases is the release count then), and an add whose hooks
// span a release is undone.
func (f *functionObservers) add(info ObserverInfo, revision uint64, namespace string, epoch uint64, releases int) {
	f.mu.Lock()
	released, existing := f.releasedSinceLocked(releases), f.known[info.ID]
	f.mu.Unlock()
	if released {
		return
	}
	if existing != nil {
		f.remove(info.ID)
	}
	obs, compileErr := newObserver(f, info, revision)
	if compileErr != nil {
		f.ack(info.ID, revision, namespace, epoch, &ErrorBody{Code: string(filter.CodeInvalidFilter), Message: compileErr.Error()})
		return
	}
	f.mu.Lock()
	obs.adding = true
	f.mu.Unlock()
	for _, level := range []hookLevel{workerLevel, functionLevel} {
		hook, gen := f.client.addedHookGen(f.functionID, level)
		obs.addedGen[level] = gen
		if hook == nil {
			continue
		}
		outcome := f.runHook(func(ctx context.Context) error { return hook(context.WithValue(ctx, addingKey{}, obs), obs) })
		if outcome.body == nil {
			obs.setUp[level] = true
			continue
		}
		f.refuse(obs, revision, namespace, epoch, outcome.body)
		if outcome.late != nil {
			// It may still finish and set something up: undo that once it does.
			go func() {
				if err := <-outcome.late; err == nil {
					f.enqueue(func() { f.runRemoved(obs, level) })
				}
			}()
		}
		return
	}
	// The keys are computed once, for the match hook installed now: only this queue
	// replaces it.
	if match := f.state.Load().match; match.byKeys() {
		keys, rejection := f.observerKeys(match, obs)
		if rejection != nil {
			f.refuse(obs, revision, namespace, epoch, rejection)
			return
		}
		f.mu.Lock()
		obs.keys = keys
		f.mu.Unlock()
	}
	// adding ends in the same critical section as the insert: a Reject is either seen
	// below or finds the observer known.
	f.mu.Lock()
	obs.adding = false
	if f.releasedSinceLocked(releases) {
		// released (a conflict, a withdrawal, an unregister) while its hooks ran, even
		// if registered again since: as removed, no ack
		f.mu.Unlock()
		obs.settle(errObserverRemoved)
		f.tearDown(obs)
		return
	}
	f.known[obs.id] = obs
	// Rejected (obs.Reject) while a hook was still running: the rejection wins, and the
	// removed hook of each level whose added hook resolved runs now (the engine sends no
	// observerremoved for an observer it never saw acked). It stays known, so a replay
	// that lists it again gets the rejection again (reack).
	if rejection := obs.rejection; rejection != nil {
		f.mu.Unlock()
		f.ack(obs.id, revision, namespace, epoch, rejection)
		obs.settle(&ObserveRejectionError{Code: rejection.Code, Message: rejection.Message})
		f.tearDown(obs)
		return
	}
	// Accepted: set up at both levels, so its removal runs the removed hooks of both.
	obs.setUp = [2]bool{true, true}
	// In the active set before the ack: the first call after "active" is delivered.
	f.activateLocked(obs)
	f.mu.Unlock()
	f.ack(obs.id, revision, namespace, epoch, nil)
	obs.settle(nil)
}

// refuse rejects an observer whose add failed with body: it is acked with the error,
// Accepted returns it, and the removed hook of each level whose added hook resolved
// runs.
func (f *functionObservers) refuse(obs *Observer, revision uint64, namespace string, epoch uint64, body *ErrorBody) {
	f.mu.Lock()
	obs.adding = false
	f.mu.Unlock()
	f.ack(obs.id, revision, namespace, epoch, body)
	obs.settle(&ObserveRejectionError{Code: body.Code, Message: body.Message})
	f.tearDown(obs)
}

// reack acks again, at revision and without running any hook, an observer this SDK
// holds that the engine announced again as info (decision 44): one with the same args,
// served from the ack on with the filters, target, capture and on it was announced with,
// or, when listed by functionregistered, one rejected late, whose rejection is sent
// again (it stays in force until a fresh observeradded starts the observer over). A
// changed view is a new *Observer sharing held's state, in held's place and filed under
// its own keys. It reports false for an observer to add as a new one after removing the
// one held: other args, a late rejection announced by observeradded, or a view this SDK
// cannot serve (filters that do not compile, keys that panic).
func (f *functionObservers) reack(held *Observer, info ObserverInfo, revision uint64, namespace string, epoch uint64, releases int, listed bool) bool {
	argsKey, viewKey := viewKeysOf(info)
	sameArgs := argsKey == held.argsKey
	// A changed view, prepared outside mu: key functions are the worker's code.
	var next *Observer
	served := true
	if sameArgs && viewKey != held.viewKey {
		next, served = f.nextView(held, info)
	}
	f.mu.Lock()
	if f.releasedSinceLocked(releases) {
		f.mu.Unlock()
		return true // released meanwhile: the release's own task removes it
	}
	if f.known[held.id] != held {
		f.mu.Unlock()
		return false
	}
	rejection := held.rejection
	switch {
	case rejection != nil:
		if !listed {
			f.mu.Unlock()
			return false
		}
	case !sameArgs, !served:
		f.mu.Unlock()
		return false
	case next != nil:
		f.known[held.id] = next
		if held.active.Load() {
			f.swapLocked(func(st *callState) {
				observers := slices.Clone(st.observers)
				for i, obs := range observers {
					if obs == held {
						observers[i] = next
					}
				}
				st.observers = observers
				if st.index != nil {
					st.index = st.index.without(held, held.keys).with(next, next.keys)
				}
				next.seq = held.seq
				st.filters = f.fileLocked(st.filters.without(held, held.seq, held.cond.Load()), next)
			})
		}
	}
	held.revision = revision
	f.mu.Unlock()
	f.ack(held.id, revision, namespace, epoch, rejection)
	return true
}

// nextView is held with the view info, sharing its state, and filed under the keys of
// that view on a function matched by keys; false when this SDK cannot serve the view.
// It runs on the hook queue, where the match hook is installed.
func (f *functionObservers) nextView(held *Observer, info ObserverInfo) (*Observer, bool) {
	next, err := observerView(f, info)
	if err != nil {
		return nil, false
	}
	next.observerState = held.observerState
	if match := f.state.Load().match; match.byKeys() {
		keys, rejection := f.observerKeys(match, next)
		if rejection != nil {
			return nil, false
		}
		next.keys = keys
	}
	return next, true
}

func (f *functionObservers) remove(id string) {
	f.mu.Lock()
	obs := f.known[id]
	if obs == nil {
		f.mu.Unlock()
		return
	}
	delete(f.known, id)
	f.deactivateLocked(obs)
	f.mu.Unlock()
	obs.settle(errObserverRemoved)
	f.tearDown(obs)
}

// tearDown runs the removed hook of each level obs is set up at, the function's before
// the worker's, and marks the level torn down: each level's removed hook runs once per
// observer, whichever of a removal, a late rejection or a refusal comes first. It runs
// on the function's queue.
func (f *functionObservers) tearDown(obs *Observer) {
	for _, level := range []hookLevel{functionLevel, workerLevel} {
		if !obs.setUp[level] {
			continue
		}
		obs.setUp[level] = false
		f.runRemoved(obs, level)
	}
}

// runRemoved runs the removed hook of level for obs; a failure is logged.
func (f *functionObservers) runRemoved(obs *Observer, level hookLevel) {
	hook := f.client.removedHook(f.functionID, level)
	if hook == nil {
		return
	}
	outcome := f.runHook(func(ctx context.Context) error {
		hook(ctx, obs)
		return nil
	})
	if outcome.body != nil {
		log.Printf("iii: OnObserverRemoved failed for observer %s of %q: %s", obs.id, f.functionID, outcome.body.Message)
	}
}

// catchUp runs the added hook of level, on the queue, for each active observer that
// was accepted without it. An error late-rejects that observer. One warning names the
// function and how many observers were active without the hook.
func (f *functionObservers) catchUp(level hookLevel) {
	f.enqueue(func() {
		hook, gen := f.client.addedHookGen(f.functionID, level)
		if hook == nil {
			return
		}
		var behind []*Observer
		for _, obs := range f.compiledSet() {
			if obs.addedGen[level] < gen {
				behind = append(behind, obs)
			}
		}
		if len(behind) == 0 {
			return
		}
		if level == workerLevel {
			log.Printf("iii: OnObserverAdded was set while %d observers of %q were active without it; running it for each of them", len(behind), f.functionID)
		} else {
			log.Printf("iii: OnObserverAddedFor(%q) was set while %d observers were active without it; running it for each of them", f.functionID, len(behind))
		}
		for _, obs := range behind {
			obs.addedGen[level] = gen
			if !obs.active.Load() {
				continue
			}
			if outcome := f.runHook(func(ctx context.Context) error { return hook(ctx, obs) }); outcome.body != nil {
				obs.Reject(outcome.body.Code, outcome.body.Message)
			}
		}
	})
}

type hookOutcome struct {
	// body is the rejection; nil when the hook succeeded.
	body *ErrorBody
	// late delivers the hook's result after a timeout.
	late <-chan error
}

func (f *functionObservers) runHook(fn func(ctx context.Context) error) hookOutcome {
	timeout := f.client.observeHookTimeout()
	ctx, cancel := context.WithTimeout(f.client.lifeCtx, timeout)
	done := make(chan error, 1)
	go func() {
		var err error
		defer func() {
			if r := recover(); r != nil {
				err = fmt.Errorf("observe hook panicked: %v", r)
			}
			done <- err
		}()
		err = fn(ctx)
	}()
	select {
	case err := <-done:
		cancel()
		if err == nil {
			return hookOutcome{}
		}
		return hookOutcome{body: rejectionBody(err)}
	case <-ctx.Done():
		late := make(chan error, 1)
		go func() {
			err := <-done
			cancel()
			late <- err
		}()
		return hookOutcome{
			body: &ErrorBody{Code: "OWNER_TIMEOUT", Message: fmt.Sprintf("observe hook of %s still running after %s", f.functionID, timeout)},
			late: late,
		}
	}
}

func rejectionBody(err error) *ErrorBody {
	var rej *ObserveRejectionError
	if errors.As(err, &rej) {
		return &ErrorBody{Code: rej.Code, Message: rej.Message}
	}
	return &ErrorBody{Code: "OBSERVE_REJECTED", Message: err.Error()}
}

func onAccepts(on On, ok bool) bool {
	switch on {
	case OnComplete:
		return true
	case OnSuccess:
		return ok
	case OnError:
		return !ok
	}
	return false
}

func (f *functionObservers) currentNamespace() string {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.namespace
}

// Observer is an observer as the worker that hosts the observed function sees it (in
// its hooks and in [Client.Observers]): its args, filters, target, capture and on.
// Never its metadata, policy or the observing worker. Its fields are set when it is
// added; the SDK keeps its own copy of what it reads, so changing them changes nothing
// but the host's view. The pointer is stable while the engine announces the same view
// of the observer, so hosts keep it in their own index (expression -> observers, route
// table).
//
// The engine may announce an observer again with the same args and other filters,
// target, capture or on (a replay): it stays the same observer and no hook runs. From
// then on it is served, and listed by [Client.Observers], as a new *Observer with that
// view; Reject and Accepted act on the observer through either pointer, and pointers
// handed out earlier keep the view they were made with.
type Observer struct {
	ID string
	// FunctionID is the observed function (the one this worker hosts).
	FunctionID string
	// Args are the observer's args, validated against args_format by the engine
	// (numbers decode as float64; see DecodeArgs).
	Args map[string]any
	// Filters are the observer's filters in normal form; iii's delivery and Matches
	// apply them.
	Filters map[string]any
	// Target is the target function id.
	Target  string
	Capture []Capture
	// On selects the calls the observer receives by status.
	On On

	owner *functionObservers
	// The SDK's own copies of what it reads: the exported fields are the host's.
	id      string
	target  string
	capture []Capture
	on      On
	argsRaw json.RawMessage
	filter  *filter.Filter
	// argsKey and viewKey identify the engine's view of the observer (viewKeysOf): its
	// args, and the rest, to tell a replay with the same args and a changed view.
	argsKey, viewKey string
	// keys are what this view of it is filed under on a function matched by keys
	// (guarded by owner.mu).
	keys []string
	// seq is its place in the function's active set, the order of activation: a view
	// that replaces it takes its seq (set under owner.mu before the view is active).
	seq uint64
	// cond is the condition this view is filed under in the function's filter index,
	// nil while it is not filed or is filed in the list every call scans. It is a
	// condition of its own filter (filterCond).
	cond atomic.Pointer[filterCond]
	// observerState is where the observer stands, shared by every view of it.
	*observerState
}

// observerState is where an observer stands, whatever view of it an *Observer holds.
type observerState struct {
	active atomic.Bool
	// addedGen is, per level, the generation of the added hook it was accepted or
	// caught up with (read and written on the function's queue only).
	addedGen [2]uint64
	// setUp is, per level, whether the observer is set up there and not torn down yet:
	// its added hook resolved, or the observer was accepted. tearDown clears it as it
	// runs the level's removed hook (read and written on the function's queue only).
	setUp [2]bool
	// guarded by owner.mu
	revision  uint64
	rejection *ErrorBody
	// adding: its added hooks are running (a Reject made meanwhile wins over them).
	adding bool

	// settled is closed once the observer is accepted (settleErr nil), or rejected or
	// removed first.
	settled    chan struct{}
	settleOnce sync.Once
	settleErr  error
}

// newObserver is a new observer with the view info, announced at revision.
func newObserver(f *functionObservers, info ObserverInfo, revision uint64) (*Observer, error) {
	obs, err := observerView(f, info)
	if err != nil {
		return nil, err
	}
	obs.observerState = &observerState{revision: revision, settled: make(chan struct{})}
	return obs, nil
}

// observerView is the view info of an observer of f, without its state.
func observerView(f *functionObservers, info ObserverInfo) (*Observer, error) {
	capture, on := viewDefaults(info)
	obs := &Observer{
		ID:         info.ID,
		FunctionID: f.functionID,
		Target:     info.Target,
		Capture:    capture,
		On:         on,
		owner:      f,
	}
	obs.id, obs.target, obs.capture, obs.on = info.ID, info.Target, slices.Clone(capture), on
	obs.argsRaw = objectOrEmpty(info.Args)
	_ = json.Unmarshal(obs.argsRaw, &obs.Args)
	filtersRaw := objectOrEmpty(info.Filters)
	_ = json.Unmarshal(filtersRaw, &obs.Filters)
	obs.argsKey, obs.viewKey = viewKeysOf(info)
	// the engine pushes the normal form, compiled without the size limit
	flt, err := filter.ParseNormalized(filtersRaw)
	if err != nil {
		return nil, err
	}
	obs.filter = flt
	return obs, nil
}

// viewDefaults is the capture (a copy) and on of info, an absent one the default.
func viewDefaults(info ObserverInfo) ([]Capture, On) {
	capture, on := slices.Clone(info.Capture), info.On
	if capture == nil {
		capture = slices.Clone(defaultCapture)
	}
	if on == "" {
		on = OnSuccess
	}
	return capture, on
}

// viewKeysOf identifies the view info announces: its args, and its filters, target,
// capture and on.
func viewKeysOf(info ObserverInfo) (argsKey, viewKey string) {
	capture, on := viewDefaults(info)
	return canonicalJSON(objectOrEmpty(info.Args)),
		canonicalJSON(objectOrEmpty(info.Filters)) + "\x00" + info.Target + "\x00" + fmt.Sprint(capture) + "\x00" + string(on)
}

// settle resolves Accepted, once.
func (o *Observer) settle(err error) {
	o.settleOnce.Do(func() {
		o.settleErr = err
		close(o.settled)
	})
}

// DecodeArgs unmarshals the observer's args into v (a struct, typically).
func (o *Observer) DecodeArgs(v any) error { return json.Unmarshal(o.argsRaw, v) }

// Matches reports whether call reaches the observer: its on accepts the call's status
// and its filters match the call. A deliver hook that calls targets itself checks it
// before calling through the observer; iii's delivery applies it on its own. On a call
// the SDK hands a hook, the observers share the call's decoded document and its values
// at the paths of the function's filter index, so each costs its own filter, and an
// observer costs a comparison when the call fails the condition it is filed under.
func (o *Observer) Matches(call *Call) bool {
	if call == nil || o.filter == nil {
		return false
	}
	if h := call.hook; h != nil {
		if v := h.viewOf(); v.reads(call) {
			return v.matches(o)
		}
	}
	return onAccepts(o.on, call.succeeded()) && o.filter.Matches(call.doc())
}

// Accepted waits until the SDK has acked the observer as accepted, and returns nil. A
// call made after that reaches an active observer, since the engine handles a
// connection's frames in order: a host sends a new observer its initial state this way,
// with a call of its own function whose metadata is {"for_observer": <obs.ID>}. The
// function's [MatchHook] gives that call to this observer alone, and iii delivers it
// like every call. It returns an *[ObserveRejectionError] when the observer is rejected
// first, an error when it is removed first, and ctx's error when ctx is done first.
// Once accepted, it returns nil at once.
//
// The ack waits for the observer's added hooks to return, so an added hook that sends
// initial state starts a goroutine that waits on Accepted and then calls the function
// for the observer:
//
//	client.OnObserverAddedFor("prices::on-change", func(ctx context.Context, obs *iii.Observer) error {
//		go func() {
//			if obs.Accepted(context.Background()) == nil {
//				meta, _ := json.Marshal(map[string]string{"for_observer": obs.ID})
//				_, _ = client.Call(context.Background(), iii.CallRequest{
//					FunctionID: "prices::on-change", Payload: prices.Current(), Metadata: meta, Action: iii.VoidAction(),
//				})
//			}
//		}()
//		return nil
//	})
//	client.OnObserverMatchFor("prices::on-change", iii.MatchFunc(func(obs *iii.Observer, call *iii.Call) bool {
//		var meta struct{ ForObserver string `json:"for_observer"` }
//		_ = json.Unmarshal(call.Metadata, &meta)
//		return meta.ForObserver == "" || meta.ForObserver == obs.ID
//	}))
//
// Called with the ctx of the observer's own added hook (or one derived from it), which
// ends when the hook returns, it returns an error at once.
func (o *Observer) Accepted(ctx context.Context) error {
	if o.observerState == nil || o.settled == nil {
		return errors.New("iii: Accepted on an observer not handed out by the client")
	}
	if ctx.Value(addingKey{}) == o {
		select {
		case <-o.settled:
			return o.settleErr
		default:
			return errAcceptedInAddedHook
		}
	}
	select {
	case <-o.settled:
		return o.settleErr
	case <-ctx.Done():
		return ctx.Err()
	}
}

// Reject sends a late rejection for this observer (e.g. after a config reload removed
// what it needs). It leaves the compiled set at once, so calls stop reaching it, and
// its removed hooks run next on the function's hook queue, connected or not; the
// engine's observerremoved that follows runs none again. The rejection is sent again
// when the engine replays the observer after a reconnect. Called while the observer's
// added hooks run, it rejects the observer once they return, and the removed hook of
// each level whose added hook resolved runs.
func (o *Observer) Reject(code, message string) {
	f := o.owner
	if f == nil {
		return
	}
	f.mu.Lock()
	if o.adding {
		// the add acks with it once the hooks return
		o.rejection = &ErrorBody{Code: code, Message: message}
		f.mu.Unlock()
		return
	}
	// o, or the view of the same observer it is served with now
	current := f.known[o.id]
	if current == nil || current.observerState != o.observerState {
		f.mu.Unlock()
		return
	}
	o.rejection = &ErrorBody{Code: code, Message: message}
	f.deactivateLocked(current)
	revision, namespace, epoch := o.revision, f.namespace, f.confirmedEpoch
	f.mu.Unlock()
	if epoch >= 0 {
		f.ack(o.id, revision, namespace, uint64(epoch), &ErrorBody{Code: code, Message: message})
	}
	f.enqueue(func() { f.tearDown(current) })
}

// objectOrEmpty maps an absent or null JSON value to {}.
func objectOrEmpty(raw json.RawMessage) json.RawMessage {
	trimmed := bytes.TrimSpace(raw)
	if len(trimmed) == 0 || bytes.Equal(trimmed, []byte("null")) {
		return json.RawMessage("{}")
	}
	return raw
}

// canonicalJSON re-encodes a JSON value with sorted keys and numbers as written.
func canonicalJSON(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	v, err := decodeJSONValue(raw)
	if err != nil {
		return string(raw)
	}
	out, err := encodeCompact(v)
	if err != nil {
		return string(raw)
	}
	return string(out)
}

// decodeJSONValue decodes JSON keeping numbers as json.Number (the filter evaluator
// compares them as doubles).
func decodeJSONValue(raw []byte) (any, error) {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		return nil, err
	}
	return v, nil
}

// lazyDoc is the document of a call, decoded root by root on first use and shared by
// every observer evaluated on it, from any goroutine.
type lazyDoc struct {
	input, output, metadata json.RawMessage
	errBody                 *ErrorBody
	caller                  *Caller

	parts [3]docPart
}

// docPart is one decoded part of a lazyDoc.
type docPart struct {
	once  sync.Once
	value any
	ok    bool
}

func (d *lazyDoc) part(i int, raw json.RawMessage) (any, bool) {
	if raw == nil {
		return nil, false
	}
	p := &d.parts[i]
	p.once.Do(func() {
		v, err := decodeJSONValue(raw)
		p.value, p.ok = v, err == nil
	})
	return p.value, p.ok
}

// Root implements filter.Doc.
func (d *lazyDoc) Root(root filter.Root) (any, bool) {
	switch root {
	case filter.RootInput:
		return d.part(0, d.input)
	case filter.RootOutput:
		return d.part(1, d.output)
	case filter.RootMetadata:
		return d.part(2, d.metadata)
	case filter.RootError:
		if d.errBody == nil {
			return nil, false
		}
		m := map[string]any{"code": d.errBody.Code, "message": d.errBody.Message}
		if d.errBody.Stacktrace != nil {
			m["stacktrace"] = *d.errBody.Stacktrace
		}
		return m, true
	case filter.RootCaller:
		if d.caller == nil {
			return nil, false
		}
		return callerDoc(d.caller), true
	}
	return nil, false
}
