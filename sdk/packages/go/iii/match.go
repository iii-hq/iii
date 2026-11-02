package iii

import (
	"fmt"
	"hash/maphash"
	"log"
	"maps"
	"slices"
)

// This file is matching: a function's match hook as its calls apply it (a predicate, or
// keys with an index of the active observers by key), installed on the function's hook
// queue, and the observers of a call it selects.

// setMatch installs m as the function's match hook on the hook queue, after the adds
// queued before it, so the index holds every observer those adds activate.
func (f *functionObservers) setMatch(m *MatchHook) {
	f.mu.Lock()
	f.wantMatch = m
	f.mu.Unlock()
	f.enqueue(func() { f.applyMatch(m) })
}

// applyMatch installs m, unless it was replaced meanwhile: for a hook by keys it computes
// the keys of every active observer and builds the index from them, and an observer
// whose keys panic is late-rejected. It runs on the hook queue: no observer is activated
// meanwhile, and one deactivated meanwhile is not filed.
func (f *functionObservers) applyMatch(m *MatchHook) {
	f.mu.Lock()
	if f.disposed || f.wantMatch != m {
		f.mu.Unlock()
		return
	}
	active := f.state.Load().observers
	f.mu.Unlock()
	keys := map[*Observer][]string{}
	var refused []*Observer
	var rejections []*ErrorBody
	if m.byKeys() {
		for _, obs := range active {
			k, rejection := f.observerKeys(m, obs)
			if rejection != nil {
				refused, rejections = append(refused, obs), append(rejections, rejection)
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
	var index *keyIndex
	if m.byKeys() {
		// the observers still active, in the order they were activated
		var filed []*Observer
		for _, obs := range f.state.Load().observers {
			k, ok := keys[obs]
			obs.keys = k
			if ok {
				filed = append(filed, obs)
			}
		}
		index = buildKeyIndex(filed)
	}
	f.swapLocked(func(st *callState) { st.match, st.index = m, index })
	f.mu.Unlock()
	for i, obs := range refused {
		obs.Reject(rejections[i].Code, rejections[i].Message)
	}
}

// matching returns the observers of st that want call: all of them without a match
// hook, those the predicate accepts, or those filed under one of the call's keys, each
// once. A match hook that panics counts as no match.
func (f *functionObservers) matching(st *callState, call *Call) []*Observer {
	switch {
	case st.match == nil:
		return st.observers
	case st.index != nil:
		return st.index.lookup(f.callKeys(st.match, call))
	}
	var matched []*Observer
	for _, obs := range st.observers {
		if f.wants(st.match, obs, call) {
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

// observerKeys runs the observer keys of m for obs, each key once; a panic is the
// observer's rejection, OBSERVE_REJECTED.
func (f *functionObservers) observerKeys(m *MatchHook, obs *Observer) (keys []string, rejection *ErrorBody) {
	defer func() {
		if r := recover(); r != nil {
			keys, rejection = nil, &ErrorBody{
				Code:    "OBSERVE_REJECTED",
				Message: fmt.Sprintf("the match keys of observer %s of %s panicked: %v", obs.id, f.functionID, r),
			}
		}
	}()
	for _, key := range m.observerKeys(obs) {
		if !slices.Contains(keys, key) {
			keys = append(keys, key)
		}
	}
	return keys, nil
}

// matchPanicked logs a panic of the function's predicate or call keys, once.
func (f *functionObservers) matchPanicked(r any) {
	if f.matchPanicLogged.CompareAndSwap(false, true) {
		log.Printf("iii: OnObserverMatch of %q panicked: %v; it counts as no match (logged once)", f.functionID, r)
	}
}

// keyShards is how many maps a key index is split into: filing or unfiling an observer
// copies the maps of its keys only.
const keyShards = 64

var keySeed = maphash.MakeSeed()

func keyShard(key string) int { return int(maphash.String(keySeed, key) % keyShards) }

// keyIndex is one function's active observers by key, for a match hook by keys. It is
// never changed: filing and unfiling an observer make a new index, which shares the
// shards they leave alone. A call reads the index of the call state it started with.
type keyIndex struct {
	shards [keyShards]map[string][]*Observer
	// filed is the number of observers filed (under at least one key).
	filed int
}

// buildKeyIndex is the index of observers, each filed under its keys.
func buildKeyIndex(observers []*Observer) *keyIndex {
	x := &keyIndex{}
	for _, obs := range observers {
		if len(obs.keys) > 0 {
			x.filed++
		}
		for _, key := range obs.keys {
			s := keyShard(key)
			if x.shards[s] == nil {
				x.shards[s] = map[string][]*Observer{}
			}
			x.shards[s][key] = append(x.shards[s][key], obs)
		}
	}
	return x
}

// edit returns a copy of x with the shard of each of keys copied, and calls change for
// each key with that key's bucket in the copy.
func (x *keyIndex) edit(keys []string, change func(bucket []*Observer) []*Observer) *keyIndex {
	next := *x
	var copied [keyShards]bool
	for _, key := range keys {
		s := keyShard(key)
		if !copied[s] {
			copied[s] = true
			next.shards[s] = maps.Clone(x.shards[s])
			if next.shards[s] == nil {
				next.shards[s] = map[string][]*Observer{}
			}
		}
		if bucket := change(next.shards[s][key]); len(bucket) > 0 {
			next.shards[s][key] = bucket
		} else {
			delete(next.shards[s], key)
		}
	}
	return &next
}

// with is x with obs filed under keys (unique); x itself when there is no key.
func (x *keyIndex) with(obs *Observer, keys []string) *keyIndex {
	if len(keys) == 0 {
		return x
	}
	next := x.edit(keys, func(bucket []*Observer) []*Observer { return append(slices.Clip(bucket), obs) })
	next.filed++
	return next
}

// without is x with obs, filed under keys, unfiled; x itself when obs is not filed.
func (x *keyIndex) without(obs *Observer, keys []string) *keyIndex {
	if len(keys) == 0 || !slices.Contains(x.bucket(keys[0]), obs) {
		return x
	}
	next := x.edit(keys, func(bucket []*Observer) []*Observer {
		return slices.DeleteFunc(slices.Clone(bucket), func(o *Observer) bool { return o == obs })
	})
	next.filed--
	return next
}

func (x *keyIndex) bucket(key string) []*Observer { return x.shards[keyShard(key)][key] }

// lookup returns the observers filed under any of keys, each once, in key order.
func (x *keyIndex) lookup(keys []string) []*Observer {
	var out []*Observer
	var seen map[*Observer]bool
	for _, key := range keys {
		bucket := x.bucket(key)
		if len(bucket) == 0 {
			continue
		}
		if out == nil {
			out = bucket
			continue
		}
		if seen == nil {
			seen = make(map[*Observer]bool, len(out)+len(bucket))
			for _, obs := range out {
				seen[obs] = true
			}
			out = slices.Clone(out)
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
	for _, key := range keys {
		if len(x.bucket(key)) > 0 {
			return true
		}
	}
	return false
}
