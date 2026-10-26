package filter

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
)

// fixturesDir is sdk/fixtures/filters, relative to this package.
var fixturesDir = filepath.Join("..", "..", "..", "..", "fixtures", "filters")

func readJSON(t *testing.T, file string) map[string]any {
	t.Helper()
	data, err := os.ReadFile(file)
	if err != nil {
		t.Fatalf("read %s: %v", file, err)
	}
	v, err := decodeJSON(data)
	if err != nil {
		t.Fatalf("decode %s: %v", file, err)
	}
	obj, ok := v.(map[string]any)
	if !ok {
		t.Fatalf("%s: not a JSON object", file)
	}
	return obj
}

// rawField re-encodes one member of a decoded vector, so filters reach Parse
// as the JSON text the vector holds (numbers keep their spelling).
func rawField(t *testing.T, v any) []byte {
	t.Helper()
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		t.Fatalf("encode: %v", err)
	}
	return bytes.TrimSpace(buf.Bytes())
}

func docOf(t *testing.T, v any) Map {
	t.Helper()
	obj, ok := v.(map[string]any)
	if !ok {
		t.Fatalf("doc is not an object: %v", v)
	}
	return Map(obj)
}

func vectorFiles(t *testing.T, dir string) []string {
	t.Helper()
	files, err := filepath.Glob(filepath.Join(dir, "*.json"))
	if err != nil {
		t.Fatal(err)
	}
	return files
}

func TestConformanceValidVectors(t *testing.T) {
	files := vectorFiles(t, fixturesDir)
	if len(files) < 100 {
		t.Fatalf("found %d valid vector files in %s, expected the full set", len(files), fixturesDir)
	}
	cases := 0
	for _, file := range files {
		t.Run(filepath.Base(file), func(t *testing.T) {
			vec := readJSON(t, file)
			f, err := Parse(rawField(t, vec["filters"]))
			if err != nil {
				t.Fatalf("filter rejected: %v", err)
			}
			list, _ := vec["cases"].([]any)
			if len(list) == 0 {
				t.Fatal("no cases")
			}
			for i, raw := range list {
				c := raw.(map[string]any)
				want := c["expect"].(bool)
				if got := f.Matches(docOf(t, c["doc"])); got != want {
					t.Errorf("case %d (%v): got %v, want %v; doc %s", i, c["note"], got, want, rawField(t, c["doc"]))
				}
				cases++
			}
		})
	}
	t.Logf("%d files, %d cases", len(files), cases)
}

func TestConformanceInvalidVectors(t *testing.T) {
	files := vectorFiles(t, filepath.Join(fixturesDir, "invalid"))
	if len(files) < 15 {
		t.Fatalf("found %d invalid vector files, expected the full set", len(files))
	}
	for _, file := range files {
		t.Run(filepath.Base(file), func(t *testing.T) {
			vec := readJSON(t, file)
			list, _ := vec["cases"].([]any)
			if len(list) == 0 {
				t.Fatal("no cases")
			}
			for i, raw := range list {
				c := raw.(map[string]any)
				_, err := Parse(rawField(t, c["filters"]))
				var fe *Error
				if !errors.As(err, &fe) {
					t.Errorf("case %d (%v): accepted, want %v", i, c["note"], c["code"])
					continue
				}
				if string(fe.Code) != c["code"] {
					t.Errorf("case %d (%v): code %s, want %v (%s)", i, c["note"], fe.Code, c["code"], fe.Message)
				}
			}
		})
	}
}

// The normal form the engine pushes (functionregistered / observeradded) is
// compiled like any filter and must give the same verdicts.
func TestNormalizeVectorsCompile(t *testing.T) {
	for _, file := range vectorFiles(t, filepath.Join(fixturesDir, "normalize")) {
		vec := readJSON(t, file)
		for i, raw := range vec["cases"].([]any) {
			c := raw.(map[string]any)
			if _, err := Parse(rawField(t, c["filters"])); err != nil {
				t.Errorf("%s case %d: filter rejected: %v", filepath.Base(file), i, err)
			}
			if _, err := Parse(rawField(t, c["normalized"])); err != nil {
				t.Errorf("%s case %d: normal form rejected: %v", filepath.Base(file), i, err)
			}
		}
	}
}

func TestDifferentialCorpus(t *testing.T) {
	corpus := readJSON(t, filepath.Join(fixturesDir, "diff", "corpus.json"))
	runs := corpus["runs"].([]any)
	if len(runs) == 0 {
		t.Fatal("no runs")
	}
	total, mismatches := 0, 0
	for _, r := range runs {
		run := r.(map[string]any)
		for i, raw := range run["cases"].([]any) {
			c := raw.(map[string]any)
			total++
			f, err := Parse(rawField(t, c["filters"]))
			if err != nil {
				mismatches++
				t.Errorf("seed %v case %d: filter rejected: %v", run["seed"], i, err)
				continue
			}
			if got := f.Matches(docOf(t, c["doc"])); got != c["expect"].(bool) {
				mismatches++
				t.Errorf("seed %v case %d: got %v, Rust says %v\nfilter %s\ndoc %s", run["seed"], i, got, c["expect"],
					rawField(t, c["filters"]), rawField(t, c["doc"]))
			}
		}
	}
	if total < 1000 {
		t.Fatalf("corpus holds %d cases, expected the committed runs", total)
	}
	t.Logf("%d differential cases, %d mismatches", total, mismatches)
}

func mustCompile(t *testing.T, src any) *Filter {
	t.Helper()
	f, err := Compile(src)
	if err != nil {
		t.Fatalf("compile: %v", err)
	}
	return f
}

type order struct {
	ID     string   `json:"id"`
	Amount int      `json:"amount"`
	Tags   []string `json:"tags,omitempty"`
	Secret string   `json:"-"`
	Note   *string  `json:"note,omitempty"`
}

func TestHostValuesReadAsJSON(t *testing.T) {
	// structs are read through encoding/json: tags, omitempty and "-" apply
	f := mustCompile(t, map[string]any{
		"input.amount":   map[string]any{"$gte": 500},
		"input.tags":     map[string]any{"$any": map[string]any{"$eq": "vip"}},
		"input.Secret":   map[string]any{"$exists": false},
		"input.note":     map[string]any{"$exists": false},
		"input.tags.0":   map[string]any{"$eq": "vip"},
		"input.id":       map[string]any{"$prefix": "ord_"},
		"metadata.count": map[string]any{"$eq": 3},
	})
	doc := Map{"input": order{ID: "ord_1", Amount: 750, Tags: []string{"vip"}, Secret: "x"}, "metadata": map[string]int{"count": 3}}
	if !f.Matches(doc) {
		t.Fatal("struct document did not match")
	}
	if f.Matches(Map{"input": &order{ID: "ord_1", Amount: 10}}) {
		t.Fatal("amount 10 matched $gte 500")
	}

	// every Go number type compares as a double
	eq := mustCompile(t, map[string]any{"input": map[string]any{"$eq": 1}})
	for _, v := range []any{int8(1), uint64(1), float32(1), 1.0, json.Number("1.0"), json.RawMessage("1e0")} {
		if !eq.Matches(Map{"input": v}) {
			t.Errorf("%T(%v) != 1", v, v)
		}
	}
	// a float32 is read as its JSON spelling
	if !mustCompile(t, map[string]any{"input": map[string]any{"$eq": 0.1}}).Matches(Map{"input": float32(0.1)}) {
		t.Error("float32(0.1) should read as 0.1")
	}
	// NaN and Inf read as null; a func is absent; in an array it is null
	null := mustCompile(t, map[string]any{"input": map[string]any{"$eq": nil}})
	if !null.Matches(Map{"input": math.NaN()}) || !null.Matches(Map{"input": math.Inf(1)}) {
		t.Error("NaN / Inf should read as null")
	}
	exists := mustCompile(t, map[string]any{"input.f": map[string]any{"$exists": true}})
	if exists.Matches(Map{"input": map[string]any{"f": func() {}}}) {
		t.Error("a func member should be absent")
	}
	elem := mustCompile(t, map[string]any{"input.0": map[string]any{"$eq": nil}})
	if !elem.Matches(Map{"input": []any{func() {}}}) {
		t.Error("a func element should read as null")
	}
	// a nil root is null, a missing root is absent
	if !null.Matches(Map{"input": nil}) || null.Matches(Map{}) || null.Matches(nil) {
		t.Error("nil root is null, missing root is absent")
	}
	// typed nil pointers, maps and slices read as null, as encoding/json writes them
	for _, v := range []any{(*order)(nil), []any(nil), map[string]any(nil), json.RawMessage(nil)} {
		if !null.Matches(Map{"input": v}) {
			t.Errorf("%T(nil) should read as null", v)
		}
	}
	nested := mustCompile(t, map[string]any{"input": map[string]any{"$eq": map[string]any{"a": nil, "b": []any{nil}}}})
	if !nested.Matches(Map{"input": map[string]any{"a": []any(nil), "b": []any{map[string]any(nil)}}}) {
		t.Error("typed nils inside a value should equal null")
	}
	in := mustCompile(t, map[string]any{"input": map[string]any{"$in": []any{nil}}})
	if !in.Matches(Map{"input": []any(nil)}) {
		t.Error("$in [null] on a nil slice")
	}
	// a host json.Number reads as encoding/json would write it: out of range is null,
	// an invalid spelling is refused (absent; null as an array element)
	if !null.Matches(Map{"input": json.Number("1e400")}) {
		t.Error("1e400 should read as null")
	}
	for _, bad := range []json.Number{"NaN", "+1", "1.", ".5", "0x10", ""} {
		if exists := mustCompile(t, map[string]any{"input": map[string]any{"$exists": true}}); exists.Matches(Map{"input": bad}) {
			t.Errorf("json.Number(%q) should be absent", bad)
		}
		if !elem.Matches(Map{"input": []any{bad}}) {
			t.Errorf("json.Number(%q) as an element should be null", bad)
		}
	}
}

func TestStringOrderIsCodePointOrder(t *testing.T) {
	// U+1F600 sorts after U+FF61 by code point, before it by UTF-16 code unit
	gt := mustCompile(t, map[string]any{"input": map[string]any{"$gt": "｡"}})
	if !gt.Matches(Map{"input": "😀"}) {
		t.Error(`"😀" > "｡" must hold`)
	}
	// invalid UTF-8 in a host string reads as encoding/json writes it (U+FFFD)
	eq := mustCompile(t, map[string]any{"input": map[string]any{"$eq": "a�"}})
	if !eq.Matches(Map{"input": "a\xff"}) {
		t.Error("invalid UTF-8 should read as U+FFFD")
	}
}

func TestCompileInputs(t *testing.T) {
	// JSON text and Go values validate the same way
	for _, src := range []any{
		json.RawMessage(`{"input.op":{"$in":["insert","update"]}}`),
		[]byte(`{"input.op":{"$in":["insert","update"]}}`),
		map[string]any{"input.op": map[string]any{"$in": []string{"insert", "update"}}},
	} {
		f := mustCompile(t, src)
		if !f.Matches(Map{"input": map[string]any{"op": "insert"}}) {
			t.Errorf("%T: no match", src)
		}
		if !f.Roots().Input || f.Roots().Output {
			t.Errorf("%T: roots %+v", src, f.Roots())
		}
	}
	for _, src := range []any{nil, "x", []any{}, json.RawMessage(`null`), json.RawMessage(`{} {}`), map[string]any{"input.op": "insert"}} {
		_, err := Compile(src)
		var fe *Error
		if !errors.As(err, &fe) || fe.Code != CodeInvalidFilter {
			t.Errorf("%#v: got %v, want INVALID_FILTER", src, err)
		}
	}
	if !MatchAll().IsMatchAll() || !mustCompile(t, map[string]any{}).IsMatchAll() {
		t.Error("{} is match-all")
	}
	if mustCompile(t, map[string]any{"$and": []any{map[string]any{}}}).Matches(nil) != true {
		t.Error("$and of {} matches everything")
	}
}

func TestRootsAndAnyElementPaths(t *testing.T) {
	f := mustCompile(t, map[string]any{
		"caller.worker_name": map[string]any{"$ne": "x"},
		"$or": []any{
			map[string]any{"error": map[string]any{"$exists": true}},
			map[string]any{"input.items": map[string]any{"$any": map[string]any{"@.sku": map[string]any{"$eq": "A"}}}},
		},
	})
	r := f.Roots()
	if !r.Caller || !r.Error || !r.Input || r.Output || r.Metadata {
		t.Errorf("roots %+v", r)
	}
	if !r.Has(RootCaller) || r.Has(RootMetadata) {
		t.Error("Has")
	}
}

func TestEncodedSizeCountsUTF8Bytes(t *testing.T) {
	// 4-byte characters: 2047 of them plus the envelope stays under 8192 bytes,
	// 2048 go over (a UTF-16 count would accept both).
	under := `{"input.s":{"$eq":"` + strings.Repeat("😀", 2040) + `"}}`
	over := `{"input.s":{"$eq":"` + strings.Repeat("😀", 2045) + `"}}`
	if _, err := Parse([]byte(under)); err != nil {
		t.Errorf("under the limit: %v", err)
	}
	if _, err := Parse([]byte(over)); err == nil {
		t.Error("over the limit was accepted")
	}
	// whitespace does not count; encoding/json's HTML escaping does not either
	spaced := []byte(`{ "input.s" : { "$eq" : "` + strings.Repeat("<", 8160) + `" } }`)
	if _, err := Parse(spaced); err != nil {
		t.Errorf("compact size is %d+: %v", 8160, err)
	}
}

func TestDeterministicErrors(t *testing.T) {
	src := []byte(`{"input.a":{"$bad":1},"output.b":"literal","zzz":{"$eq":1}}`)
	_, first := Parse(src)
	for i := 0; i < 50; i++ {
		_, err := Parse(src)
		if err.Error() != first.Error() {
			t.Fatalf("error changed between runs: %v vs %v", err, first)
		}
	}
}

// The fuzz target calls the evaluator without Matches' recover, so a panic of the
// evaluator itself fails it.
func FuzzNeverPanics(f *testing.F) {
	f.Add([]byte(`{"input.a":{"$eq":1}}`), []byte(`{"input":{"a":1}}`))
	f.Add([]byte(`{"input.a":{"$any":{"@.b":{"$gt":"x"}}}}`), []byte(`{"input":{"a":[{"b":"y"},1,null]}}`))
	f.Add([]byte(`{"$or":[{"input.a.0":{"$in":[1,"x",null]}},{"$not":{"error":{"$exists":true}}}]}`), []byte(`{"input":{"a":[1e400]},"error":null}`))
	f.Fuzz(func(t *testing.T, filterJSON, docJSON []byte) {
		flt, err := Parse(filterJSON)
		if err != nil {
			return
		}
		doc, err := decodeJSON(docJSON)
		if err != nil {
			return
		}
		if m, ok := doc.(map[string]any); ok && !flt.matchAll {
			flt.matches(Map(m))
		}
	})
}

// The size limit measures numbers by their value, not their spelling: 768 numbers
// spelled with 25 zeros are 21 KB as written and 3 KB canonical (the engine accepts).
func TestEncodedSizeMeasuresNumbersByValue(t *testing.T) {
	items := strings.TrimSuffix(strings.Repeat("1.0000000000000000000000000,", 256), ",")
	src := `{"input.a":{"$in":[` + items + `]},"input.b":{"$in":[` + items + `]},"input.c":{"$nin":[` + items + `]}}`
	if len(src) < 20000 {
		t.Fatalf("the raw filter is only %d bytes", len(src))
	}
	f, err := Parse([]byte(src))
	if err != nil {
		t.Fatalf("rejected by its spelling: %v", err)
	}
	if !f.Matches(Map{"input": map[string]any{"a": 1, "b": 1.0}}) {
		t.Error("no match")
	}
	for spelled, canonical := range map[string]string{"1.0": "1", "-0": "0", "1e21": "1e+21", "1E-7": "1e-7", "0.5e1": "5", "12345678901234567890": "12345678901234567890"} {
		if got := canonicalNumber(spelled); got != canonical {
			t.Errorf("canonical %s = %s, want %s", spelled, got, canonical)
		}
	}
}

// Every normal form (normalize/*.json) agrees with its source on the documents of the
// valid vectors and of the differential corpus.
func TestNormalFormsAgreeWithTheirSources(t *testing.T) {
	var docs []Map
	for _, file := range vectorFiles(t, fixturesDir) {
		for _, raw := range readJSON(t, file)["cases"].([]any) {
			docs = append(docs, docOf(t, raw.(map[string]any)["doc"]))
		}
	}
	corpus := readJSON(t, filepath.Join(fixturesDir, "diff", "corpus.json"))
	for _, r := range corpus["runs"].([]any) {
		for _, raw := range r.(map[string]any)["cases"].([]any) {
			docs = append(docs, docOf(t, raw.(map[string]any)["doc"]))
		}
	}
	checked := 0
	for _, file := range vectorFiles(t, filepath.Join(fixturesDir, "normalize")) {
		for i, raw := range readJSON(t, file)["cases"].([]any) {
			c := raw.(map[string]any)
			src, err := Parse(rawField(t, c["filters"]))
			if err != nil {
				t.Fatalf("%s case %d: %v", filepath.Base(file), i, err)
			}
			norm, err := ParseNormalized(rawField(t, c["normalized"]))
			if err != nil {
				t.Fatalf("%s case %d normal form: %v", filepath.Base(file), i, err)
			}
			for j, doc := range docs {
				if src.matches(doc) != norm.matches(doc) {
					t.Errorf("%s case %d: source and normal form disagree on doc %d", filepath.Base(file), i, j)
					break
				}
				checked++
			}
		}
	}
	t.Logf("%d (normal form, document) pairs agree", checked)
}

// One compiled filter is shared by every goroutine evaluating it (go test -race).
func TestSharedFilterIsSafeForConcurrentUse(t *testing.T) {
	f := mustCompile(t, map[string]any{
		"input.items": map[string]any{"$any": map[string]any{"@.sku": map[string]any{"$prefix": "A-"}}},
		"$or":         []any{map[string]any{"caller.worker_name": map[string]any{"$ne": "x"}}, map[string]any{"error": map[string]any{"$exists": true}}},
	})
	var wg sync.WaitGroup
	for g := 0; g < 8; g++ {
		wg.Add(1)
		go func(g int) {
			defer wg.Done()
			for i := 0; i < 500; i++ {
				doc := Map{"input": map[string]any{"items": []any{map[string]any{"sku": fmt.Sprintf("A-%d", i)}}}, "caller": order{ID: "o"}}
				if !f.Matches(doc) {
					t.Errorf("goroutine %d: no match", g)
					return
				}
				_ = f.Source()
			}
		}(g)
	}
	wg.Wait()
}

func TestSourceIsACopy(t *testing.T) {
	MatchAll().Source()["input.x"] = 1
	if len(MatchAll().Source()) != 0 || !MatchAll().Matches(nil) {
		t.Error("MatchAll's source is shared")
	}
	f := mustCompile(t, map[string]any{"input.a": map[string]any{"$in": []any{1}}})
	f.Source()["input.a"].(map[string]any)["$in"] = []any{2}
	if got := f.Source()["input.a"].(map[string]any)["$in"].([]any)[0]; got != json.Number("1") {
		t.Errorf("source changed through Source(): %v", got)
	}
}

func TestLoneSurrogatesAndInvalidUTF8AreRejected(t *testing.T) {
	for _, src := range []string{
		`{"input.a":{"$eq":"\ud83d"}}`,
		`{"input.a":{"$eq":"\ude00x"}}`,
		`{"input.a":{"$eq":"\ud83d\ud83d"}}`,
		`{"input.\ud83d":{"$exists":true}}`,
		"{\"input.a\":{\"$eq\":\"\xff\"}}",
	} {
		_, err := Parse([]byte(src))
		var fe *Error
		if !errors.As(err, &fe) || fe.Code != CodeInvalidFilter {
			t.Errorf("%s: got %v, want INVALID_FILTER", src, err)
		}
	}
	// a surrogate pair is a code point
	f, err := Parse([]byte(`{"input.a":{"$eq":"😀"}}`))
	if err != nil || !f.Matches(Map{"input": map[string]any{"a": "😀"}}) {
		t.Errorf("surrogate pair: %v", err)
	}
	// escaped backslashes before a 'u' are not escapes
	if _, err := Parse([]byte(`{"input.a":{"$eq":"\\ud83d"}}`)); err != nil {
		t.Errorf(`"\\ud83d" is a plain string: %v`, err)
	}
}

func TestParseNormalizedSkipsTheSizeLimit(t *testing.T) {
	big := `{"$and":[{"input.s":{"$eq":"` + strings.Repeat("x", 9000) + `"}}]}`
	if _, err := Parse([]byte(big)); err == nil {
		t.Fatal("Parse accepted a filter over 8 KiB")
	}
	f, err := ParseNormalized([]byte(big))
	if err != nil {
		t.Fatalf("ParseNormalized: %v", err)
	}
	if !f.Matches(Map{"input": map[string]any{"s": strings.Repeat("x", 9000)}}) {
		t.Error("no match")
	}
	// every other rule still applies
	if _, err := ParseNormalized([]byte(`{"input.a":"literal"}`)); err == nil {
		t.Error("ParseNormalized accepted a literal shorthand")
	}
	if _, err := ParseNormalized([]byte(`[]`)); err == nil {
		t.Error("ParseNormalized accepted a non-object")
	}
}

type panicky struct{}

func (panicky) MarshalJSON() ([]byte, error) { panic("boom") }

func TestMatchesIsTotal(t *testing.T) {
	f := mustCompile(t, map[string]any{"input.a": map[string]any{"$eq": 1}})
	if f.Matches(Map{"input": panicky{}}) {
		t.Error("a panicking Marshaler matched")
	}
	cyclic := map[string]any{}
	cyclic["a"] = cyclic
	deep := mustCompile(t, map[string]any{"input.a.a.a.a": map[string]any{"$eq": map[string]any{"a": map[string]any{}}}})
	if deep.Matches(Map{"input": cyclic}) {
		t.Error("a cyclic document matched a finite literal")
	}
}
