package iii

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math/rand/v2"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/iii-hq/iii/sdk/packages/go/iii/filter"
)

// The filter index selects exactly the observers an evaluation of every observer's on
// and filters selects, in the same order (decision 55): random filters over every
// operator and random documents (types, numbers spelled differently, null and missing
// paths, arrays, nested paths), an equality-heavy corpus with shared paths and
// respelled values, churn with every version kept, every conformance vector in one
// index, and the differential corpus. Observer.Matches through a call's view agrees
// with Observer.Matches on the bare call.

var filterFixturesDir = filepath.Join("..", "..", "..", "fixtures", "filters")

// indexed is observers filed in one index, in seq order.
type indexed struct {
	f         *functionObservers
	index     *filterIndex
	observers []*Observer
	seq       uint64
}

func newIndexed() *indexed {
	return &indexed{f: newFunctionObservers(New("ws://unused.invalid"), "idx::fn", true, false)}
}

// add files an observer with filters (JSON text) and on; false when the filters do not
// compile.
func (x *indexed) add(t testing.TB, id, filters string, on On) bool {
	t.Helper()
	obs, err := newObserver(x.f, ObserverInfo{ID: id, Target: "x::y", Filters: json.RawMessage(filters), On: on}, 1)
	if err != nil {
		return false
	}
	x.seq++
	obs.seq = x.seq
	var cond *filterCond
	x.index, cond = x.index.with(obs, obs.seq)
	obs.cond.Store(cond)
	x.observers = append(x.observers, obs)
	return true
}

func (x *indexed) remove(i int) {
	obs := x.observers[i]
	x.index = x.index.without(obs, obs.seq, obs.cond.Load())
	x.observers = slices.Delete(slices.Clone(x.observers), i, i+1)
}

// viaIndex is the ids of the observers the index selects for doc, a call of status ok.
func viaIndex(index *filterIndex, doc filter.Doc, ok bool) (ids []string, candidates int) {
	index.each(doc, func(obs *Observer) {
		candidates++
		if onAccepts(obs.on, ok) && obs.filter.Matches(doc) {
			ids = append(ids, obs.id)
		}
	})
	return ids, candidates
}

// byScan is the ids of the observers whose on and filters match doc.
func byScan(observers []*Observer, doc filter.Doc, ok bool) []string {
	var ids []string
	for _, obs := range observers {
		if onAccepts(obs.on, ok) && obs.filter.Matches(doc) {
			ids = append(ids, obs.id)
		}
	}
	return ids
}

// gen makes random filters and calls.
type gen struct{ r *rand.Rand }

func (g gen) pick(options ...string) string { return options[g.r.IntN(len(options))] }

// paths read every root, nested keys, array elements by index, and keys that are
// digits.
var genPaths = []string{
	"input.a", "input.b", "input.a.b", "input.a.k", "input.arr", "input.arr.0", "input.arr.1",
	"input.n", "input.s", "input.tenant", "input.0", "input.a.0",
	"output.x", "output.a", "output.n", "metadata.tenant", "metadata.a",
	"caller.function_id", "caller.worker_name", "caller.parent.function_id", "error.code", "error.message",
}

// scalars are literals as written: numbers equal in several spellings, -0, 2^53,
// strings that look like numbers.
var genScalars = []string{
	`null`, `true`, `false`, `0`, `-0`, `0.0`, `1`, `1.0`, `1e0`, `10e-1`, `2`, `-1`, `0.5`, `9007199254740992`,
	`"a"`, `"b"`, `""`, `"1"`, `"true"`, `"null"`, `"tenant-1"`, `"tenant-2"`, `"é"`,
}

var genCompound = []string{`[]`, `[1]`, `[1,2]`, `["a"]`, `{}`, `{"k":1}`, `{"k":1.0}`, `[null]`}

func (g gen) literal() string {
	if g.r.IntN(6) == 0 {
		return g.pick(genCompound...)
	}
	return g.pick(genScalars...)
}

func (g gen) literals(scalarOnly bool) string {
	n := g.r.IntN(4)
	items := make([]string, n)
	for i := range items {
		if scalarOnly {
			items[i] = g.pick(genScalars...)
		} else {
			items[i] = g.literal()
		}
	}
	return "[" + strings.Join(items, ",") + "]"
}

// operators is one operator object; equality says how often $eq / $in come up (in
// tenths).
func (g gen) operators(depth, equality int) string {
	n := 1 + g.r.IntN(2)
	used := map[string]bool{}
	var ops []string
	for range n {
		var op string
		if g.r.IntN(10) < equality {
			op = g.pick("$eq", "$in")
		} else {
			op = g.pick("$eq", "$ne", "$in", "$nin", "$gt", "$gte", "$lt", "$lte", "$exists", "$prefix", "$any")
		}
		if used[op] || (op == "$any" && depth >= 3) {
			continue
		}
		used[op] = true
		var operand string
		switch op {
		case "$eq", "$ne":
			operand = g.literal()
		case "$in":
			operand = g.literals(g.r.IntN(4) != 0)
		case "$nin":
			operand = g.literals(false)
		case "$gt", "$gte", "$lt", "$lte":
			operand = g.pick(`0`, `1`, `1.5`, `-1`, `"a"`, `"b"`, `""`)
		case "$exists":
			operand = g.pick(`true`, `false`)
		case "$prefix":
			operand = g.pick(`""`, `"a"`, `"tenant-"`, `"t"`, `"1"`)
		case "$any":
			if g.r.IntN(2) == 0 {
				operand = g.operators(depth+1, equality)
			} else {
				operand = `{"@` + g.pick("", ".k", ".a", ".0") + `":` + g.operators(depth+1, equality) + `}`
			}
		}
		ops = append(ops, fmt.Sprintf("%q:%s", op, operand))
	}
	return "{" + strings.Join(ops, ",") + "}"
}

// filter is a random filter at depth (1 is the top level).
func (g gen) filter(depth, equality int) string {
	n := 1 + g.r.IntN(3)
	used := map[string]bool{}
	var clauses []string
	for range n {
		kind := g.r.IntN(10)
		switch {
		case kind < 7 || depth >= 3:
			path := g.pick(genPaths...)
			if used[path] {
				continue
			}
			used[path] = true
			clauses = append(clauses, fmt.Sprintf("%q:%s", path, g.operators(depth, equality)))
		case kind == 7 && !used["$and"]:
			used["$and"] = true
			clauses = append(clauses, `"$and":[`+g.filter(depth, equality)+`,`+g.filter(depth, equality)+`]`)
		case kind == 8 && !used["$or"]:
			used["$or"] = true
			clauses = append(clauses, `"$or":[`+g.filter(depth+1, equality)+`,`+g.filter(depth+1, equality)+`]`)
		case !used["$not"]:
			used["$not"] = true
			clauses = append(clauses, `"$not":`+g.filter(depth+1, equality))
		}
	}
	return "{" + strings.Join(clauses, ",") + "}"
}

// value is a random document value: scalars, arrays and objects of the generator's
// keys.
func (g gen) value(depth int) string {
	switch k := g.r.IntN(10); {
	case k < 6 || depth >= 3:
		return g.pick(genScalars...)
	case k < 8:
		n := g.r.IntN(4)
		items := make([]string, n)
		for i := range items {
			items[i] = g.value(depth + 1)
		}
		return "[" + strings.Join(items, ",") + "]"
	default:
		return g.object(depth + 1)
	}
}

func (g gen) object(depth int) string {
	var members []string
	for _, key := range []string{"a", "b", "k", "arr", "n", "s", "tenant", "x", "0"} {
		if g.r.IntN(3) == 0 {
			members = append(members, fmt.Sprintf("%q:%s", key, g.value(depth)))
		}
	}
	return "{" + strings.Join(members, ",") + "}"
}

// part is a random root: absent, or a document value (mostly an object).
func (g gen) part() json.RawMessage {
	switch g.r.IntN(6) {
	case 0:
		return nil
	case 1:
		return json.RawMessage(g.value(1))
	}
	return json.RawMessage(g.object(1))
}

func (g gen) call() *Call {
	call := &Call{FunctionID: "idx::fn", InvocationID: "inv", Input: g.part(), Metadata: g.part()}
	if g.r.IntN(4) == 0 {
		call.Status = "error"
		call.Error = &ErrorBody{Code: g.pick("CARD_DECLINED", "a", ""), Message: g.pick("declined", "b")}
	} else {
		call.Status = "success"
		call.Output = g.part()
	}
	if g.r.IntN(3) != 0 {
		call.Caller = &Caller{WorkerName: g.pick("checkout", "a"), FunctionID: g.pick("a", "b", "orders::create")}
		if g.r.IntN(2) == 0 {
			call.Caller.Parent = &Caller{FunctionID: g.pick("a", "b")}
		}
	}
	return call
}

func (g gen) on() On { return []On{OnSuccess, OnError, OnComplete}[g.r.IntN(3)] }

// checkCall compares the index with a scan for call, and the view's Matches with the
// bare call's for every observer.
func checkCall(t *testing.T, x *indexed, call *Call, what string) bool {
	t.Helper()
	ok := call.succeeded()
	doc := call.doc()
	got, _ := viaIndex(x.index, doc, ok)
	want := byScan(x.observers, doc, ok)
	if !slices.Equal(got, want) {
		in, _ := json.Marshal(call)
		t.Errorf("%s: index %v, scan %v\ncall %s", what, got, want, in)
		return false
	}
	view := newCallView(call, x.index)
	call.view = view
	for _, obs := range x.observers {
		bare := *call
		bare.view = nil
		if got, want := obs.Matches(call), obs.Matches(&bare); got != want {
			t.Errorf("%s: Matches through the view %v, on the bare call %v; observer %s filters %v", what, got, want, obs.id, obs.Filters)
			return false
		}
	}
	return true
}

func TestFilterIndexMatchesAScanOfRandomFilters(t *testing.T) {
	for seed := range uint64(40) {
		g := gen{rand.New(rand.NewPCG(seed, 55))}
		x := newIndexed()
		// equality-heavy in half the seeds
		equality := 3
		if seed%2 == 1 {
			equality = 8
		}
		compiled := 0
		for i := range 60 {
			if x.add(t, fmt.Sprintf("o-%d", i), g.filter(1, equality), g.on()) {
				compiled++
			}
		}
		if compiled < 40 {
			t.Fatalf("seed %d: %d of 60 random filters compiled", seed, compiled)
		}
		for i := range 300 {
			if !checkCall(t, x, g.call(), fmt.Sprintf("seed %d call %d", seed, i)) {
				return
			}
		}
	}
}

// Equality conditions on a few shared paths, values spelled in several ways, filters
// with more than one condition (only one is indexed), $in sharing values with $eq.
func TestFilterIndexEqualityCorpus(t *testing.T) {
	g := gen{rand.New(rand.NewPCG(7, 55))}
	x := newIndexed()
	paths := []string{"input.tenant", "input.a", "metadata.tenant", "input.a.b"}
	values := []string{`1`, `1.0`, `1e0`, `0`, `-0`, `"1"`, `true`, `null`, `"t-1"`, `"t-2"`, `"t-3"`, `2`}
	for i := range 400 {
		var clauses []string
		used := map[string]bool{}
		for range 1 + g.r.IntN(3) {
			path := g.pick(paths...)
			if used[path] {
				continue
			}
			used[path] = true
			if g.r.IntN(3) == 0 {
				clauses = append(clauses, fmt.Sprintf(`{%q:{"$in":[%s,%s]}}`, path, g.pick(values...), g.pick(values...)))
			} else {
				clauses = append(clauses, fmt.Sprintf(`{%q:{"$eq":%s}}`, path, g.pick(values...)))
			}
		}
		if !x.add(t, fmt.Sprintf("e-%d", i), `{"$and":[`+strings.Join(clauses, ",")+`]}`, OnComplete) {
			t.Fatalf("filter %d did not compile", i)
		}
	}
	for i := range 2000 {
		input := map[string]json.RawMessage{}
		for _, key := range []string{"tenant", "a"} {
			if g.r.IntN(5) != 0 {
				input[key] = json.RawMessage(g.pick(values...))
			}
		}
		if g.r.IntN(4) == 0 {
			input["a"] = json.RawMessage(`{"b":` + g.pick(values...) + `}`)
		}
		raw, _ := json.Marshal(input)
		call := &Call{Status: "success", Input: raw, Output: json.RawMessage(`{}`), Metadata: json.RawMessage(`{"tenant":` + g.pick(values...) + `}`)}
		if !checkCall(t, x, call, fmt.Sprintf("call %d", i)) {
			return
		}
	}
}

// Every version an index went through stays the index of the observers it held then.
func TestFilterIndexChurnKeepsEveryVersion(t *testing.T) {
	g := gen{rand.New(rand.NewPCG(11, 55))}
	x := newIndexed()
	type version struct {
		index     *filterIndex
		observers []*Observer
	}
	var versions []version
	next := 0
	for range 400 {
		if len(x.observers) > 0 && g.r.IntN(5) < 2 {
			x.remove(g.r.IntN(len(x.observers)))
		} else {
			next++
			x.add(t, fmt.Sprintf("c-%d", next), g.filter(1, 7), g.on())
		}
		versions = append(versions, version{x.index, x.observers})
	}
	for i, v := range versions {
		if got := v.index.len(); got != len(v.observers) {
			t.Fatalf("version %d holds %d observers, want %d", i, got, len(v.observers))
		}
		for range 20 {
			call := g.call()
			doc := call.doc()
			got, _ := viaIndex(v.index, doc, call.succeeded())
			if want := byScan(v.observers, doc, call.succeeded()); !slices.Equal(got, want) {
				t.Fatalf("version %d: index %v, scan %v", i, got, want)
			}
		}
	}
	// unfiling every observer empties the index
	for len(x.observers) > 0 {
		x.remove(g.r.IntN(len(x.observers)))
	}
	if x.index.len() != 0 || len(x.index.paths) != 0 || len(x.index.scanned) != 0 {
		t.Errorf("empty index %+v", x.index)
	}
}

// readFilterJSON decodes a fixture keeping numbers as written.
func readFilterJSON(t *testing.T, file string) map[string]any {
	t.Helper()
	data, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	v, err := decodeJSONValue(data)
	if err != nil {
		t.Fatal(err)
	}
	return v.(map[string]any)
}

func rawJSON(t *testing.T, v any) string {
	t.Helper()
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		t.Fatal(err)
	}
	return strings.TrimSpace(buf.String())
}

// Every conformance vector's filter in one index: each case's document selects exactly
// the filters a scan selects, and its own filter as the vector expects.
func TestFilterIndexConformanceVectors(t *testing.T) {
	files, _ := filepath.Glob(filepath.Join(filterFixturesDir, "*.json"))
	if len(files) < 100 {
		t.Fatalf("found %d vector files", len(files))
	}
	x := newIndexed()
	type vectorCase struct {
		id     string
		doc    filter.Map
		expect bool
	}
	var cases []vectorCase
	for _, file := range files {
		vec := readFilterJSON(t, file)
		id := filepath.Base(file)
		if !x.add(t, id, rawJSON(t, vec["filters"]), OnComplete) {
			t.Fatalf("%s: filter rejected", id)
		}
		for _, raw := range vec["cases"].([]any) {
			c := raw.(map[string]any)
			cases = append(cases, vectorCase{id, filter.Map(c["doc"].(map[string]any)), c["expect"].(bool)})
		}
	}
	for i, c := range cases {
		got, _ := viaIndex(x.index, c.doc, true)
		if want := byScan(x.observers, c.doc, true); !slices.Equal(got, want) {
			t.Fatalf("case %d (%s): index %v, scan %v", i, c.id, got, want)
		}
		if slices.Contains(got, c.id) != c.expect {
			t.Errorf("case %d (%s): matched %v, want %v", i, c.id, slices.Contains(got, c.id), c.expect)
		}
	}
	t.Logf("%d vectors, %d cases", len(x.observers), len(cases))
}

// The differential corpus, each run's filters in one index.
func TestFilterIndexDiffCorpus(t *testing.T) {
	corpus := readFilterJSON(t, filepath.Join(filterFixturesDir, "diff", "corpus.json"))
	total := 0
	for _, r := range corpus["runs"].([]any) {
		run := r.(map[string]any)
		x := newIndexed()
		var docs []filter.Map
		var expect []bool
		for i, raw := range run["cases"].([]any) {
			c := raw.(map[string]any)
			if !x.add(t, fmt.Sprint(i), rawJSON(t, c["filters"]), OnComplete) {
				t.Fatalf("seed %v case %d: filter rejected", run["seed"], i)
			}
			docs = append(docs, filter.Map(c["doc"].(map[string]any)))
			expect = append(expect, c["expect"].(bool))
		}
		for i, doc := range docs {
			total++
			got, _ := viaIndex(x.index, doc, true)
			if want := byScan(x.observers, doc, true); !slices.Equal(got, want) {
				t.Fatalf("seed %v case %d: index %v, scan %v", run["seed"], i, got, want)
			}
			if slices.Contains(got, fmt.Sprint(i)) != expect[i] {
				t.Errorf("seed %v case %d: its own filter matched %v, Rust says %v", run["seed"], i, !expect[i], expect[i])
			}
		}
	}
	if total < 1000 {
		t.Fatalf("%d cases", total)
	}
}

// What the index files an observer under, and what it leaves to the scanned list: an
// $eq of a scalar or an $in of scalars on a path of the top-level $and (an explicit $and
// included), never one inside $or, $not or $any, nor an $eq of an array or an object.
// Numbers equal as doubles share a value, -0 included; a string or a boolean never
// equals a number. A call with a value at an indexed path evaluates the observers filed
// under that value only.
func TestFilterIndexFilesEqualityConditions(t *testing.T) {
	x := newIndexed()
	for _, tc := range []struct {
		filters string
		filed   bool
	}{
		{`{"$and":[{"input.tenant":{"$eq":1}}]}`, true},
		{`{"input.tenant":{"$eq":1.0}}`, true},
		{`{"input.tenant":{"$in":[2,"x",null]}}`, true},
		{`{"$and":[{"$and":[{"input.tenant":{"$eq":-0}}]}]}`, true},
		{`{"input.tenant":{"$gt":0,"$eq":true}}`, true},
		{`{"input.tenant":{"$in":[1,[1]]}}`, false},
		{`{"input.tenant":{"$in":[]}}`, false},
		{`{"input.tenant":{"$eq":[1]}}`, false},
		{`{"input.tenant":{"$eq":{"k":1}}}`, false},
		{`{"$or":[{"input.tenant":{"$eq":1}},{"input.tenant":{"$eq":2}}]}`, false},
		{`{"$not":{"input.tenant":{"$eq":1}}}`, false},
		{`{"input.arr":{"$any":{"$eq":1}}}`, false},
		{`{"input.tenant":{"$ne":1}}`, false},
		{`{"input.tenant":{"$nin":[1]}}`, false},
		{`{}`, false},
	} {
		id := tc.filters
		if !x.add(t, id, tc.filters, OnComplete) {
			t.Fatalf("%s did not compile", tc.filters)
		}
		obs := x.observers[len(x.observers)-1]
		if filed := obs.cond.Load() != nil; filed != tc.filed {
			t.Errorf("%s: filed %v, want %v", tc.filters, filed, tc.filed)
		}
	}
	candidates := func(input string) int {
		_, n := viaIndex(x.index, (&Call{Status: "success", Input: json.RawMessage(input)}).doc(), true)
		return n
	}
	scanned := len(x.index.scanned)
	// 1, 1.0 and -0 / 0 are numbers; "1" and true are not
	for _, tc := range []struct {
		input string
		want  int
	}{
		{`{"tenant":1}`, 2},    // $eq 1, $eq 1.0
		{`{"tenant":1e0}`, 2},  // the same number
		{`{"tenant":0}`, 1},    // $eq -0
		{`{"tenant":"x"}`, 1},  // the $in
		{`{"tenant":null}`, 1}, // the $in
		{`{"tenant":true}`, 1}, // $gt 0 and $eq true
		{`{"tenant":"1"}`, 0},  // a string is not a number
		{`{"tenant":[1]}`, 0},  // an array equals no scalar
		{`{"other":1}`, 0},     // absent
	} {
		if got := candidates(tc.input); got != tc.want+scanned {
			t.Errorf("%s: %d candidates, want %d filed and the %d scanned", tc.input, got, tc.want, scanned)
		}
		call := &Call{Status: "success", Input: json.RawMessage(tc.input)}
		checkCall(t, x, call, tc.input)
	}

	// 1,000 observers on their own tenant: a call evaluates one
	many := newIndexed()
	for i := range 1000 {
		many.add(t, fmt.Sprint(i), fmt.Sprintf(`{"$and":[{"input.tenant":{"$eq":"tenant-%d"}}]}`, i), OnSuccess)
	}
	ids, n := viaIndex(many.index, (&Call{Status: "success", Input: json.RawMessage(`{"tenant":"tenant-500"}`)}).doc(), true)
	if !slices.Equal(ids, []string{"500"}) || n != 1 {
		t.Errorf("matched %v with %d candidates, want [500] with 1", ids, n)
	}
	if _, n := viaIndex(many.index, (&Call{Status: "success", Input: json.RawMessage(`{"tenant":"nobody"}`)}).doc(), true); n != 0 {
		t.Errorf("%d candidates for a tenant nobody observes", n)
	}
}

// The observer with the fewest observers sharing its value is filed under that
// condition: a filter on a shared type and its own tenant is filed under the tenant.
func TestFilterIndexFilesUnderTheLeastSharedCondition(t *testing.T) {
	x := newIndexed()
	for i := range 50 {
		x.add(t, fmt.Sprint(i), fmt.Sprintf(`{"$and":[{"input.kind":{"$eq":"order"}},{"input.tenant":{"$eq":%d}}]}`, i), OnSuccess)
	}
	_, n := viaIndex(x.index, (&Call{Status: "success", Input: json.RawMessage(`{"kind":"order","tenant":7}`)}).doc(), true)
	if n > 2 {
		t.Errorf("%d candidates: the shared condition was indexed", n)
	}
	checkCall(t, x, &Call{Status: "success", Input: json.RawMessage(`{"kind":"order","tenant":7}`)}, "kind and tenant")
}
