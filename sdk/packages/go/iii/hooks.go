package iii

import (
	"context"
	"encoding/json"
)

// This file is what a worker does when its functions are observed: the four hooks, set
// for one function by id (on the client or on its FunctionRef) or, for added and
// removed, for every function the worker hosts; and the Call the match and deliver
// hooks read.

// AddedHook runs when an observer is added to a function this worker hosts, before the
// SDK acks it. Returning nil accepts the observer; an error built with
// [ObserveRejection] rejects it with that code, any other error with OBSERVE_REJECTED.
// ctx is cancelled after 30 s, when the observer fails with OWNER_TIMEOUT.
type AddedHook func(ctx context.Context, obs *Observer) error

// RemovedHook runs once for every accepted observer when it is removed (unobserve,
// disposal, late rejection with [Observer.Reject]). For an observer rejected before it
// is accepted (by another level's [AddedHook], or by [Observer.Reject] while the added
// hooks run), it runs for each level whose [AddedHook] resolved. A panic is logged; the
// observer is removed either way.
type RemovedHook func(ctx context.Context, obs *Observer)

// MatchHook says which observers want each call of one function; without one, every
// observer wants every call. Build it with [MatchFunc] (a predicate) or [MatchKeys]
// (keys the SDK indexes). The observers it matches are what iii delivers the call to,
// after their on and filters, or what the function's [DeliverHook] receives. A nil
// MatchHook, and the zero one, match every observer.
//
// A host that sends one observer something of its own (its initial state) calls the
// function with metadata {"for_observer": <observer id>}, and the hook gives such a
// call to that observer alone: a predicate compares the ids, and observerKeys also files
// each observer under "for_observer:<id>", the one key callKeys gives such a call.
type MatchHook struct {
	predicate    func(*Observer, *Call) bool
	observerKeys func(*Observer) []string
	callKeys     func(*Call) []string
}

// MatchFunc is a match hook that asks predicate, once per active observer per call,
// whether the observer wants the call. A predicate that panics counts as no match, and
// the SDK logs it once per function. A nil predicate gives a nil MatchHook.
//
//	// an observer of expression X gets the ticks of expression X
//	client.OnObserverMatchFor("cron::on-tick", iii.MatchFunc(func(obs *iii.Observer, call *iii.Call) bool {
//		var tick struct{ Expression string `json:"expression"` }
//		_ = json.Unmarshal(call.Input, &tick)
//		return obs.Args["expression"] == tick.Expression
//	}))
func MatchFunc(predicate func(obs *Observer, call *Call) bool) *MatchHook {
	if predicate == nil {
		return nil
	}
	return &MatchHook{predicate: predicate}
}

// MatchKeys is a match hook by keys: an observer wants a call when they share a key, and
// the SDK keeps an index of the active observers by key instead of asking each of them.
// observerKeys says what an observer listens for: the SDK computes it once, when the
// observer is accepted, files the observer under each key and unfiles it when it is
// removed. An observer whose observerKeys panics is rejected with OBSERVE_REJECTED.
// callKeys says what a call is: the call matches the observers filed under any of its
// keys, each once; a callKeys that panics matches nothing, and the SDK logs it once per
// function. Keys express wildcards by listing them: an observer of every table is filed
// under "primary|*|insert", and a change matches "primary|orders|insert",
// "primary|orders|*", "primary|*|insert" and "primary|*|*". A nil function gives a nil
// MatchHook.
//
// On a function matched by keys, [Client.IsObservedFor] answers with one lookup per
// call key.
func MatchKeys(observerKeys func(obs *Observer) []string, callKeys func(call *Call) []string) *MatchHook {
	if observerKeys == nil || callKeys == nil {
		return nil
	}
	return &MatchHook{observerKeys: observerKeys, callKeys: callKeys}
}

// set reports whether m narrows the observers: a predicate, or key functions.
func (m *MatchHook) set() bool {
	return m != nil && (m.predicate != nil || (m.observerKeys != nil && m.callKeys != nil))
}

// byKeys reports whether m matches by keys.
func (m *MatchHook) byKeys() bool {
	return m != nil && m.predicate == nil && m.observerKeys != nil && m.callKeys != nil
}

// DeliverHook delivers each call of one function itself. It receives the call and the
// observers that matched it ([MatchHook], or every active observer), as they were when
// the call started. What it returns is the caller's result, encoded as a handler's is;
// the error it returns is the caller's error. iii delivers nothing for the function:
// the hook calls a target through its observer ([CallRequest].Observer) and applies the
// observer's on and filters with [Observer.Matches]. It runs inside the function's
// invocation, so calls made with ctx have that invocation as their caller.
type DeliverHook func(ctx context.Context, call *Call, observers []*Observer) (any, error)

// Call is one call of a function, as its host sees it in a [MatchHook], a [DeliverHook]
// and [Observer.Matches]. The fields of a call the SDK hands over are read-only.
type Call struct {
	FunctionID   string
	InvocationID string
	// Status is "success" or "error". Empty reads as "error" when Error is set, and
	// "success" otherwise.
	Status string
	// Input is the call's input, as the handler received it.
	Input json.RawMessage
	// Output is the handler's return value as JSON: what observers observe. Nil on an
	// error.
	Output json.RawMessage
	// Error is the error the handler returned. Nil on success.
	Error *ErrorBody
	// Caller is who made the call; nil for a call made outside any handler.
	Caller *Caller
	// Metadata is the call's metadata sidecar; nil when it carries none.
	Metadata json.RawMessage
}

// succeeded reports the call's status.
func (call *Call) succeeded() bool {
	switch call.Status {
	case "success":
		return true
	case "error":
		return false
	}
	return call.Error == nil
}

// doc is the call as filters read it.
func (call *Call) doc() *lazyDoc {
	doc := &lazyDoc{input: call.Input, caller: call.Caller, metadata: call.Metadata}
	if call.succeeded() {
		doc.output = call.Output
	} else {
		doc.errBody = call.Error
	}
	return doc
}

// Identity is a handler that returns the input it is called with. An event source
// registers it: its observers observe each event as both input and output.
func Identity(_ context.Context, data json.RawMessage) (any, error) { return data, nil }

// observerHooks are the hooks of one level: one function, or every function.
type observerHooks struct {
	added AddedHook
	// addedGen counts the settings of added: an observer accepted with an older
	// generation is caught up with the hook (catchUp).
	addedGen uint64
	removed  RemovedHook
	match    *MatchHook
	deliver  DeliverHook
}

func (h *observerHooks) empty() bool {
	return h == nil || (h.added == nil && h.removed == nil && h.match == nil && h.deliver == nil)
}

// hookLevel names a level of the added and removed hooks.
type hookLevel int

const (
	workerLevel hookLevel = iota
	functionLevel
)

// OnObserverAdded sets the hook that runs when an observer is added to any function
// this worker hosts, before the function's own; nil removes it. A hook set while
// functions have active observers runs for each of them at once (see
// [Client.OnObserverAddedFor]). It returns the client, for chaining.
func (c *Client) OnObserverAdded(hook AddedHook) *Client {
	c.mu.Lock()
	c.hookGen++
	c.workerHooks.added, c.workerHooks.addedGen = hook, c.hookGen
	var functions []*functionObservers
	if hook != nil {
		for _, fn := range c.functions {
			functions = append(functions, fn.observe)
		}
	}
	c.mu.Unlock()
	for _, f := range functions {
		f.catchUp(workerLevel)
	}
	return c
}

// OnObserverAddedFor sets the hook that runs when an observer is added to functionID,
// after the worker's; nil removes it. Hooks are stored by function id: set them before
// RegisterFunction, so they run for every observer, and they stay across
// [FunctionRef.Unregister] and a later registration of the id.
//
// A hook set once the function already has active observers is caught up: it runs for
// each of them at once, an error late-rejects that observer, and the SDK logs one
// warning naming the function and how many observers were active without it. It
// returns the client, for chaining.
func (c *Client) OnObserverAddedFor(functionID string, hook AddedHook) *Client {
	f := c.setFunctionHook(functionID, func(h *observerHooks, _ *functionObservers) {
		c.hookGen++
		h.added, h.addedGen = hook, c.hookGen
	})
	if f != nil && hook != nil {
		f.catchUp(functionLevel)
	}
	return c
}

// OnObserverRemoved sets the hook that runs when an observer is removed from any
// function this worker hosts, after the function's own; nil removes it. It returns the
// client, for chaining.
func (c *Client) OnObserverRemoved(hook RemovedHook) *Client {
	c.mu.Lock()
	c.workerHooks.removed = hook
	c.mu.Unlock()
	return c
}

// OnObserverRemovedFor sets the hook that runs when an observer is removed from
// functionID, before the worker's; nil removes it. It is stored by function id, as
// [Client.OnObserverAddedFor] is. It returns the client, for chaining.
func (c *Client) OnObserverRemovedFor(functionID string, hook RemovedHook) *Client {
	c.setFunctionHook(functionID, func(h *observerHooks, _ *functionObservers) { h.removed = hook })
	return c
}

// OnObserverMatchFor sets which observers want each call of functionID: [MatchFunc] or
// [MatchKeys]; nil restores the default, where every observer wants every call. It is
// stored by id, as [Client.OnObserverAddedFor] is. A match hook set on a registered
// function takes effect on the function's hook queue, after the hooks queued before it,
// and calls use the previous one until then. A hook by keys set while the function has
// active observers rebuilds the index from them; an observer whose keys panic is
// late-rejected with OBSERVE_REJECTED. A function with a match hook needs an engine with
// observe.v2: against an older one it stays unregistered, and the SDK logs UNSUPPORTED
// once. It returns the client, for chaining.
func (c *Client) OnObserverMatchFor(functionID string, m *MatchHook) *Client {
	if !m.set() {
		m = nil
	}
	c.setFunctionHook(functionID, func(h *observerHooks, f *functionObservers) {
		h.match = m
		if f != nil {
			f.setMatch(m)
		}
	})
	c.reconsiderRegistration(functionID)
	return c
}

// OnObserverDeliverFor sets the hook that delivers each call of functionID itself; nil
// restores iii's delivery, which delivers each call to every matched observer whose on
// and filters accept it, and gives the caller the handler's result. It is stored by id,
// as [Client.OnObserverAddedFor] is. A function with a deliver hook needs an engine with
// observe.v2: against an older one it stays unregistered, and the SDK logs UNSUPPORTED
// once. A deliver hook does not run for a function registered with
// [Client.RegisterHTTPFunction]: the engine delivers its calls. It returns the client,
// for chaining.
func (c *Client) OnObserverDeliverFor(functionID string, hook DeliverHook) *Client {
	c.setFunctionHook(functionID, func(h *observerHooks, f *functionObservers) {
		h.deliver = hook
		if f != nil {
			f.setDeliver(hook)
		}
	})
	c.reconsiderRegistration(functionID)
	return c
}

// setFunctionHook applies set to functionID's hooks, under mu, with the function's
// observe state when it is registered (nil otherwise), and returns that state.
func (c *Client) setFunctionHook(functionID string, set func(*observerHooks, *functionObservers)) *functionObservers {
	c.mu.Lock()
	defer c.mu.Unlock()
	h := c.functionHooks[functionID]
	if h == nil {
		h = &observerHooks{}
		c.functionHooks[functionID] = h
	}
	var f *functionObservers
	if fn, ok := c.functions[functionID]; ok {
		f = fn.observe
	}
	set(h, f)
	if h.empty() {
		delete(c.functionHooks, functionID)
	} else if f == nil && c.registeredOnConnection {
		c.checkHookIDsLater(c.epoch)
	}
	return f
}

// addedHookGen is the added hook of one level for functionID, and its generation.
func (c *Client) addedHookGen(functionID string, level hookLevel) (AddedHook, uint64) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if level == workerLevel {
		return c.workerHooks.added, c.workerHooks.addedGen
	}
	if h := c.functionHooks[functionID]; h != nil {
		return h.added, h.addedGen
	}
	return nil, 0
}

// removedHook is the removed hook of one level for functionID.
func (c *Client) removedHook(functionID string, level hookLevel) RemovedHook {
	c.mu.Lock()
	defer c.mu.Unlock()
	if level == workerLevel {
		return c.workerHooks.removed
	}
	if h := c.functionHooks[functionID]; h != nil {
		return h.removed
	}
	return nil
}

// OnObserverAdded sets the function's added hook, as [Client.OnObserverAddedFor] does
// for its id, and returns the handle, for chaining.
func (r *FunctionRef) OnObserverAdded(hook AddedHook) *FunctionRef {
	if r != nil && r.client != nil {
		r.client.OnObserverAddedFor(r.ID, hook)
	}
	return r
}

// OnObserverRemoved sets the function's removed hook, as [Client.OnObserverRemovedFor]
// does for its id, and returns the handle, for chaining.
func (r *FunctionRef) OnObserverRemoved(hook RemovedHook) *FunctionRef {
	if r != nil && r.client != nil {
		r.client.OnObserverRemovedFor(r.ID, hook)
	}
	return r
}

// OnObserverMatch sets the function's match hook, as [Client.OnObserverMatchFor] does
// for its id, and returns the handle, for chaining.
func (r *FunctionRef) OnObserverMatch(m *MatchHook) *FunctionRef {
	if r != nil && r.client != nil {
		r.client.OnObserverMatchFor(r.ID, m)
	}
	return r
}

// OnObserverDeliver sets the function's deliver hook, as [Client.OnObserverDeliverFor]
// does for its id, and returns the handle, for chaining.
func (r *FunctionRef) OnObserverDeliver(hook DeliverHook) *FunctionRef {
	if r != nil && r.client != nil {
		r.client.OnObserverDeliverFor(r.ID, hook)
	}
	return r
}
