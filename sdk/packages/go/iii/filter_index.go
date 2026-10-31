package iii

import (
	"cmp"
	"encoding/json"
	"hash/maphash"
	"maps"
	"math"
	"slices"
	"sync"

	"github.com/iii-hq/iii/sdk/packages/go/iii/filter"
	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/filterlink"
)

// This file is where filters run on this worker (decision 55). The engine evaluates the
// on and filters of the routed calls it holds; a host evaluates its local calls, which
// the engine never sees, and Observer.Matches in its hooks. Both match through an index
// of the observers' equality conditions: each active observer of a function is filed
// under one $eq (or $in) condition of a scalar on a path of its filter's top-level $and,
// path -> value -> observers, and the observers without one are kept in a list. A call
// resolves each indexed path once and looks its value up; on and the whole filter are
// evaluated for the observers filed there and those of the list only. Every other
// observer has a condition the call fails, so the observers a call selects are exactly
// those an evaluation of every observer selects, in the same order (the order of the
// active set).

// filterShardBits sizes the maps the values of one indexed path are split into
// (filterShards of them): filing or unfiling an observer copies the maps of its values
// only.
const (
	filterShardBits = 6
	filterShards    = 1 << filterShardBits
)

var scalarSeed = maphash.MakeSeed()

func scalarShard(s filterlink.Scalar) int {
	switch s.Kind {
	case filterlink.String:
		return int(maphash.String(scalarSeed, s.Str) & (filterShards - 1))
	case filterlink.Number:
		return int((math.Float64bits(s.Num) * 0x9e3779b97f4a7c15) >> (64 - filterShardBits))
	}
	return int(s.Kind) + int(s.Num) // null, false, true
}

// filedObserver is an observer in a filter index, with its seq (Observer.seq): every
// list of an index is in seq order, the order of the function's active set.
type filedObserver struct {
	seq uint64
	obs *Observer
}

// filterCond is the condition an observer is filed under: its value at path is one of
// values. It is a condition of the observer's own filter, so a call whose value at path
// is none of them does not match the observer, wherever it is evaluated (callView).
type filterCond struct {
	path   *filterlink.Path
	values []filterlink.Scalar
}

func (c *filterCond) admits(v filterlink.Scalar) bool { return slices.Contains(c.values, v) }

// pathIndex is the observers filed under a condition on one path, by value.
type pathIndex struct {
	path   *filterlink.Path
	shards [filterShards]map[filterlink.Scalar][]filedObserver
	// filed is how many observers are filed here.
	filed int
}

func (pi *pathIndex) bucket(v filterlink.Scalar) []filedObserver {
	return pi.shards[scalarShard(v)][v]
}

// filterIndex is one function's active observers by the equality conditions of their
// filters. It is never changed: filing and unfiling an observer make a new index, which
// shares what they leave alone, and a call reads the index of the call state it started
// with. A nil *filterIndex is empty.
type filterIndex struct {
	paths []*pathIndex
	// scanned are the observers filed under no condition.
	scanned []filedObserver
}

// with is x with obs filed at seq, and the condition it is filed under (nil: in the
// scanned list). Of the conditions of its filter's top-level $and it takes one naming a
// single value (an $eq, or an $in of one) before one naming several, then the one the
// fewest observers already filed share, then the one naming fewer values, as
// iii_filter::FilterIndex does.
func (x *filterIndex) with(obs *Observer, seq uint64) (*filterIndex, *filterCond) {
	var next filterIndex
	if x != nil {
		next = *x
	}
	entry := filedObserver{seq: seq, obs: obs}
	at, term := next.place(obs.filter)
	if term == nil {
		next.scanned = insertFiled(next.scanned, entry)
		return &next, nil
	}
	var pi pathIndex
	if at >= 0 {
		pi = *next.paths[at]
	} else {
		pi.path = term.Path
	}
	var copied [filterShards]bool
	for _, v := range term.Values {
		s := scalarShard(v)
		if !copied[s] {
			copied[s] = true
			pi.shards[s] = maps.Clone(pi.shards[s])
			if pi.shards[s] == nil {
				pi.shards[s] = map[filterlink.Scalar][]filedObserver{}
			}
		}
		pi.shards[s][v] = insertFiled(pi.shards[s][v], entry)
	}
	pi.filed++
	paths := slices.Clone(next.paths)
	if at >= 0 {
		paths[at] = &pi
	} else {
		paths = append(paths, &pi)
	}
	next.paths = paths
	return &next, &filterCond{path: pi.path, values: term.Values}
}

// place picks the condition f is filed under: the position of its path in x.paths (-1
// for a path not indexed yet) and the term; a nil term for a filter with none.
func (x *filterIndex) place(f *filter.Filter) (int, *filterlink.Term) {
	if f == nil {
		return -1, nil
	}
	terms := filterlink.Terms(f)
	best, bestAt := -1, -1
	var bestScore [3]int
	for i := range terms {
		t := &terms[i]
		at := slices.IndexFunc(x.paths, func(pi *pathIndex) bool { return pi.path.Key == t.Path.Key })
		sharing := 0
		if at >= 0 {
			for _, v := range t.Values {
				sharing += len(x.paths[at].bucket(v))
			}
		}
		several := 0
		if len(t.Values) > 1 {
			several = 1
		}
		score := [3]int{several, sharing, len(t.Values)}
		if best < 0 || slices.Compare(score[:], bestScore[:]) < 0 {
			best, bestAt, bestScore = i, at, score
		}
	}
	if best < 0 {
		return -1, nil
	}
	return bestAt, &terms[best]
}

// without is x with obs, filed at seq under cond, unfiled; x itself when it is not filed
// there.
func (x *filterIndex) without(obs *Observer, seq uint64, cond *filterCond) *filterIndex {
	if x == nil {
		return nil
	}
	next := *x
	if cond == nil {
		list, ok := removeFiled(x.scanned, seq, obs)
		if !ok {
			return x
		}
		next.scanned = list
		return &next
	}
	at := slices.IndexFunc(x.paths, func(pi *pathIndex) bool { return pi.path == cond.path })
	if at < 0 {
		return x
	}
	pi := *x.paths[at]
	var copied [filterShards]bool
	removed := false
	for _, v := range cond.values {
		s := scalarShard(v)
		list, ok := removeFiled(pi.shards[s][v], seq, obs)
		if !ok {
			continue
		}
		removed = true
		if !copied[s] {
			copied[s] = true
			pi.shards[s] = maps.Clone(pi.shards[s])
		}
		if len(list) > 0 {
			pi.shards[s][v] = list
		} else {
			delete(pi.shards[s], v)
		}
	}
	if !removed {
		return x
	}
	pi.filed--
	paths := slices.Clone(x.paths)
	if pi.filed == 0 {
		paths = slices.Delete(paths, at, at+1)
	} else {
		paths[at] = &pi
	}
	next.paths = paths
	return &next
}

// insertFiled is a copy of list with e in seq order.
func insertFiled(list []filedObserver, e filedObserver) []filedObserver {
	i, _ := slices.BinarySearchFunc(list, e.seq, compareSeq)
	out := make([]filedObserver, 0, len(list)+1)
	out = append(out, list[:i]...)
	out = append(out, e)
	return append(out, list[i:]...)
}

// removeFiled is a copy of list without obs at seq; false when it is not there.
func removeFiled(list []filedObserver, seq uint64, obs *Observer) ([]filedObserver, bool) {
	i, found := slices.BinarySearchFunc(list, seq, compareSeq)
	if !found || list[i].obs != obs {
		return list, false
	}
	out := make([]filedObserver, 0, len(list)-1)
	out = append(out, list[:i]...)
	return append(out, list[i+1:]...), true
}

func compareSeq(e filedObserver, seq uint64) int { return cmp.Compare(e.seq, seq) }

// each calls fn, in seq order, with every observer of x that doc does not rule out: the
// observers filed under doc's value at an indexed path, and the scanned ones. Each
// indexed path of doc is resolved once.
func (x *filterIndex) each(doc filter.Doc, fn func(*Observer)) {
	if x == nil {
		return
	}
	var held [4][]filedObserver
	lists := held[:0]
	for _, pi := range x.paths {
		v, ok := pi.path.Resolve(doc)
		if !ok {
			continue
		}
		if bucket := pi.bucket(v); len(bucket) > 0 {
			lists = append(lists, bucket)
		}
	}
	if len(x.scanned) > 0 {
		lists = append(lists, x.scanned)
	}
	if len(lists) == 1 {
		for _, e := range lists[0] {
			fn(e.obs)
		}
		return
	}
	// An observer is filed in one list only: merge them by seq.
	for {
		first := -1
		for i, list := range lists {
			if len(list) > 0 && (first < 0 || list[0].seq < lists[first][0].seq) {
				first = i
			}
		}
		if first < 0 {
			return
		}
		fn(lists[first][0].obs)
		lists[first] = lists[first][1:]
	}
}

// len is how many observers x holds.
func (x *filterIndex) len() int {
	if x == nil {
		return 0
	}
	n := len(x.scanned)
	for _, pi := range x.paths {
		n += pi.filed
	}
	return n
}

// hookCall is a call the SDK hands a hook of a function with observers, allocated with
// what its view is made from: Call.hook points back here. The view is made on the first
// Observer.Matches of the call, so a hook that evaluates no filter costs nothing more
// than the Call.
type hookCall struct {
	call Call
	// filters is the filter index of the call state the call started with.
	filters *filterIndex
	once    sync.Once
	view    *callView
}

// newHookCall is a Call to hand a hook, whose view reads the paths of filters.
func newHookCall(filters *filterIndex) *Call {
	h := &hookCall{filters: filters}
	h.call.hook = h
	return &h.call
}

// viewOf is the view of the call, made on first use.
func (h *hookCall) viewOf() *callView {
	h.once.Do(func() { h.view = newCallView(&h.call, h.filters) })
	return h.view
}

// callView is a call as the filters of its function's observers read it, shared by
// every observer evaluated on it: its document, each part decoded once, and its value at
// each path of the function's filter index, resolved once. Observer.Matches on a call
// the SDK hands a hook evaluates each observer through it, at the cost of its own filter
// (an observer whose condition the call fails costs a comparison). It is safe for
// concurrent use.
type callView struct {
	succeeded bool
	doc       lazyDoc
	paths     []*filterlink.Path
	probes    []probe
}

// probe is a call's value at one indexed path, resolved once.
type probe struct {
	once  sync.Once
	value filterlink.Scalar
	ok    bool
}

// newCallView is the view of call for the observers of index.
func newCallView(call *Call, index *filterIndex) *callView {
	v := &callView{succeeded: call.succeeded()}
	call.readInto(&v.doc)
	if index != nil && len(index.paths) > 0 {
		v.paths = make([]*filterlink.Path, len(index.paths))
		for i, pi := range index.paths {
			v.paths[i] = pi.path
		}
		v.probes = make([]probe, len(index.paths))
	}
	return v
}

// reads reports whether filters read call as the call v was made for: the same status,
// and the same parts (the same bytes in memory, the same error and caller).
func (v *callView) reads(call *Call) bool {
	d := &v.doc
	if call.succeeded() != v.succeeded || !sameRaw(d.input, call.Input) || !sameRaw(d.metadata, call.Metadata) || d.caller != call.Caller {
		return false
	}
	if v.succeeded {
		return sameRaw(d.output, call.Output)
	}
	return d.errBody == call.Error
}

// sameRaw reports whether a and b are the same bytes in memory (or both nil, or both
// empty and non-nil).
func sameRaw(a, b json.RawMessage) bool {
	if len(a) != len(b) || (a == nil) != (b == nil) {
		return false
	}
	return len(a) == 0 || &a[0] == &b[0]
}

// matches reports whether obs's on accepts the call's status and its filters match the
// call. The condition obs is filed under is checked first, against the call's value at
// its path.
func (v *callView) matches(obs *Observer) bool {
	if obs.filter == nil || !onAccepts(obs.on, v.succeeded) {
		return false
	}
	if c := obs.cond.Load(); c != nil {
		if i := slices.Index(v.paths, c.path); i >= 0 {
			p := &v.probes[i]
			p.once.Do(func() { p.value, p.ok = c.path.Resolve(&v.doc) })
			if !p.ok || !c.admits(p.value) {
				return false
			}
		}
	}
	return obs.filter.Matches(&v.doc)
}
