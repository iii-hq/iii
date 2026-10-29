package iii

import (
	"fmt"
	"log"
	"slices"
	"sync"
)

// This file is matching: a function's match hook as its calls apply it (a predicate, or
// keys with an index of the active observers by key), installed on the function's hook
// queue, and the observers of a call it selects.

// matchState is a match hook as calls apply it: the hook, and for a hook by keys the
// index of the active observers by key. It is replaced, never changed, when the hook
// is; the index changes as observers are activated and deactivated.
type matchState struct {
	hook  *MatchHook
	index *keyIndex
}

// newMatchState is the state of m with an empty index, or nil when m matches every
// observer.
func newMatchState(m *MatchHook) *matchState {
	switch {
	case m.byKeys():
		return &matchState{hook: m, index: newKeyIndex()}
	case m.set():
		return &matchState{hook: m}
	}
	return nil
}

// byKeys reports whether the state matches by keys.
func (m *matchState) byKeys() bool { return m != nil && m.index != nil }

// setMatch installs m as the function's match hook on the hook queue, after the adds
// queued before it, so the index holds every observer those adds activate.
func (f *functionObservers) setMatch(m *MatchHook) {
	f.mu.Lock()
	f.wantMatch = m
	f.mu.Unlock()
	f.enqueue(func() { f.applyMatch(m) })
}

// applyMatch builds m's state from the active observers and swaps it in, unless m was
// replaced meanwhile. An observer whose keys panic is late-rejected. It runs on the hook
// queue: no observer is activated meanwhile, and one deactivated meanwhile is not filed.
func (f *functionObservers) applyMatch(m *MatchHook) {
	f.mu.Lock()
	if f.disposed || f.wantMatch != m {
		f.mu.Unlock()
		return
	}
	active := f.compiledSet()
	f.mu.Unlock()
	next := newMatchState(m)
	keys := map[*Observer][]string{}
	type refusal struct {
		obs  *Observer
		body *ErrorBody
	}
	var refused []refusal
	if next.byKeys() {
		for _, obs := range active {
			k, rejection := f.observerKeys(m, obs)
			if rejection != nil {
				refused = append(refused, refusal{obs, rejection})
				continue
			}
			keys[obs] = k
		}
	}
	f.mu.Lock()
	if f.disposed || f.wantMatch != m {
		f.mu.Unlock()
		return
	}
	for obs, k := range keys {
		next.index.file(obs, k)
	}
	st := *f.state.Load()
	st.match = next
	f.state.Store(&st)
	f.mu.Unlock()
	for _, r := range refused {
		r.obs.Reject(r.body.Code, r.body.Message)
	}
}

// matching returns the observers of st that want call: all of them without a match
// hook, those the predicate accepts, or those filed under one of the call's keys, each
// once. A match hook that panics counts as no match.
func (f *functionObservers) matching(st *callState, call *Call) []*Observer {
	m := st.match
	switch {
	case m == nil:
		return st.observers
	case m.byKeys():
		return m.index.lookup(f.callKeys(m.hook, call))
	}
	var matched []*Observer
	for _, obs := range st.observers {
		if f.wants(m.hook, obs, call) {
			matched = append(matched, obs)
		}
	}
	return matched
}

// wants runs the predicate of m for obs and call; a panic is no match.
func (f *functionObservers) wants(m *MatchHook, obs *Observer, call *Call) (ok bool) {
	defer func() {
		if r := recover(); r != nil {
			f.matchPanicked(r)
			ok = false
		}
	}()
	return m.predicate(obs, call)
}

// callKeys runs the call keys of m; a panic gives no key.
func (f *functionObservers) callKeys(m *MatchHook, call *Call) (keys []string) {
	defer func() {
		if r := recover(); r != nil {
			f.matchPanicked(r)
			keys = nil
		}
	}()
	return m.callKeys(call)
}

// observerKeys runs the observer keys of m for obs; a panic is the observer's
// rejection, OBSERVE_REJECTED.
func (f *functionObservers) observerKeys(m *MatchHook, obs *Observer) (keys []string, rejection *ErrorBody) {
	defer func() {
		if r := recover(); r != nil {
			keys, rejection = nil, &ErrorBody{
				Code:    "OBSERVE_REJECTED",
				Message: fmt.Sprintf("the match keys of observer %s of %s panicked: %v", obs.id, f.functionID, r),
			}
		}
	}()
	return m.observerKeys(obs), nil
}

// matchPanicked logs a panic of the function's predicate or call keys, once.
func (f *functionObservers) matchPanicked(r any) {
	if f.matchPanicLogged.CompareAndSwap(false, true) {
		log.Printf("iii: OnObserverMatch of %q panicked: %v; it counts as no match (logged once)", f.functionID, r)
	}
}

// keyIndex is one function's index of its active observers by key, for a match hook by
// keys. An observer is filed when it is activated and unfiled when it is deactivated,
// under the function's mu; calls read it under the index's own lock.
type keyIndex struct {
	mu    sync.RWMutex
	byKey map[string][]*Observer
	filed map[*Observer][]string
}

func newKeyIndex() *keyIndex {
	return &keyIndex{byKey: map[string][]*Observer{}, filed: map[*Observer][]string{}}
}

// file files obs under keys, each once, while it is active.
func (x *keyIndex) file(obs *Observer, keys []string) {
	x.mu.Lock()
	defer x.mu.Unlock()
	if _, done := x.filed[obs]; done || !obs.active.Load() {
		return
	}
	var unique []string
	for _, key := range keys {
		if slices.Contains(unique, key) {
			continue
		}
		unique = append(unique, key)
		x.byKey[key] = append(x.byKey[key], obs)
	}
	x.filed[obs] = unique
}

func (x *keyIndex) unfile(obs *Observer) {
	x.mu.Lock()
	defer x.mu.Unlock()
	keys, ok := x.filed[obs]
	if !ok {
		return
	}
	delete(x.filed, obs)
	for _, key := range keys {
		// lookup copies under the lock, so the bucket is edited in place
		list := slices.DeleteFunc(x.byKey[key], func(o *Observer) bool { return o == obs })
		if len(list) == 0 {
			delete(x.byKey, key)
		} else {
			x.byKey[key] = list
		}
	}
}

// lookup returns the observers filed under any of keys, each once, in key order.
func (x *keyIndex) lookup(keys []string) []*Observer {
	x.mu.RLock()
	defer x.mu.RUnlock()
	var out []*Observer
	var seen map[*Observer]bool
	for _, key := range keys {
		bucket := x.byKey[key]
		if len(bucket) == 0 {
			continue
		}
		if out == nil {
			out = append([]*Observer(nil), bucket...)
			continue
		}
		if seen == nil {
			seen = make(map[*Observer]bool, len(out)+len(bucket))
			for _, obs := range out {
				seen[obs] = true
			}
		}
		for _, obs := range bucket {
			if !seen[obs] {
				seen[obs] = true
				out = append(out, obs)
			}
		}
	}
	return out
}

// has reports whether an observer is filed under one of keys.
func (x *keyIndex) has(keys []string) bool {
	x.mu.RLock()
	defer x.mu.RUnlock()
	for _, key := range keys {
		if len(x.byKey[key]) > 0 {
			return true
		}
	}
	return false
}

// size is the number of observers filed (tests and diagnostics).
func (x *keyIndex) size() int {
	x.mu.RLock()
	defer x.mu.RUnlock()
	return len(x.filed)
}
