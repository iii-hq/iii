package filter

import (
	"path/filepath"
	"slices"
	"testing"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/filterlink"
)

// termsOf is the conditions of a filter as path key -> values.
func termsOf(t *testing.T, src string) map[string][]filterlink.Scalar {
	t.Helper()
	f, err := Parse([]byte(src))
	if err != nil {
		t.Fatalf("%s: %v", src, err)
	}
	out := map[string][]filterlink.Scalar{}
	for _, term := range filterlink.Terms(f) {
		out[term.Path.Key] = append(out[term.Path.Key], term.Values...)
	}
	return out
}

func num(f float64) filterlink.Scalar { return filterlink.Scalar{Kind: filterlink.Number, Num: f} }
func str(s string) filterlink.Scalar  { return filterlink.Scalar{Kind: filterlink.String, Str: s} }

// A filter's equality conditions are the $eq of a scalar and the $in of scalars of the
// path clauses of its top-level $and, explicit $and included; nothing below $or, $not or
// $any. Values are keyed by the language's equality and listed once.
func TestTermsAreTheTopLevelEqualityConditions(t *testing.T) {
	got := termsOf(t, `{"input.a":{"$eq":1.0,"$gt":0},"input.b":{"$in":[1,"x",null,1e0,true]},`+
		`"$and":[{"input.c":{"$eq":-0}},{"$and":[{"input.d.0":{"$eq":"y"}}]}],`+
		`"$or":[{"input.e":{"$eq":1}},{"input.e":{"$eq":2}}],"$not":{"input.f":{"$eq":1}},`+
		`"input.g":{"$any":{"$eq":1}},"input.h":{"$eq":[1]},"input.i":{"$in":[1,{"k":1}]},"input.j":{"$in":[]},`+
		`"input.k":{"$ne":1,"$nin":[1]}}`)
	want := map[string][]filterlink.Scalar{
		"input/1:a":     {num(1)},
		"input/1:b":     {num(1), str("x"), {Kind: filterlink.Null}, {Kind: filterlink.Bool, Num: 1}},
		"input/1:c":     {num(0)},
		"input/1:d/1:0": {str("y")},
	}
	if len(got) != len(want) {
		t.Fatalf("terms %v, want %v", got, want)
	}
	for key, values := range want {
		if !slices.Equal(got[key], values) {
			t.Errorf("%s: %v, want %v", key, got[key], values)
		}
	}
	if terms := termsOf(t, `{}`); len(terms) != 0 {
		t.Errorf("{} has terms %v", terms)
	}
	// -0 is 0, and the key of a path escapes nothing away
	if c := termsOf(t, `{"input.n":{"$eq":-0}}`)["input/1:n"]; len(c) != 1 || c[0] != num(0) || c[0].Num != 0 {
		t.Errorf("-0: %v", c)
	}
	if _, ok := termsOf(t, `{"input.a\\.b":{"$eq":1}}`)["input/3:a.b"]; !ok {
		t.Error("an escaped dot is part of the key")
	}
}

// Each condition is necessary: a document whose value at its path is none of its values
// does not match the filter. Checked on every conformance vector and the differential
// corpus.
func TestTermsAreNecessaryConditions(t *testing.T) {
	type pair struct {
		filters any
		doc     Map
	}
	var pairs []pair
	for _, file := range vectorFiles(t, fixturesDir) {
		vec := readJSON(t, file)
		for _, raw := range vec["cases"].([]any) {
			c := raw.(map[string]any)
			pairs = append(pairs, pair{vec["filters"], docOf(t, c["doc"])})
		}
	}
	corpus := readJSON(t, filepath.Join(fixturesDir, "diff", "corpus.json"))
	for _, r := range corpus["runs"].([]any) {
		for _, raw := range r.(map[string]any)["cases"].([]any) {
			c := raw.(map[string]any)
			pairs = append(pairs, pair{c["filters"], docOf(t, c["doc"])})
		}
	}
	ruledOut, terms := 0, 0
	for i, p := range pairs {
		f, err := Parse(rawField(t, p.filters))
		if err != nil {
			t.Fatalf("pair %d: %v", i, err)
		}
		matches := f.Matches(p.doc)
		for _, term := range filterlink.Terms(f) {
			terms++
			v, ok := term.Path.Resolve(p.doc)
			if ok && slices.Contains(term.Values, v) {
				continue
			}
			ruledOut++
			if matches {
				t.Errorf("pair %d: the document fails %s %v and matches %s; doc %s", i, term.Path.Key, term.Values, rawField(t, p.filters), rawField(t, map[string]any(p.doc)))
			}
		}
	}
	if terms < 100 || ruledOut < 50 {
		t.Fatalf("%d conditions, %d ruled out: the fixtures should exercise more", terms, ruledOut)
	}
	t.Logf("%d pairs, %d conditions, %d ruled out", len(pairs), terms, ruledOut)
}

// Resolve reads a path as the evaluator does, and is total.
func TestResolveIsTotal(t *testing.T) {
	f, err := Parse([]byte(`{"input.a":{"$eq":1}}`))
	if err != nil {
		t.Fatal(err)
	}
	path := filterlink.Terms(f)[0].Path
	for _, tc := range []struct {
		doc  any
		want filterlink.Scalar
		ok   bool
	}{
		{Map{"input": map[string]any{"a": 1}}, num(1), true},
		{Map{"input": map[string]any{"a": int8(-0)}}, num(0), true},
		{Map{"input": map[string]any{"a": []any{1}}}, filterlink.Scalar{}, false},
		{Map{"input": map[string]any{}}, filterlink.Scalar{}, false},
		{Map{"input": map[string]any{"a": nil}}, filterlink.Scalar{Kind: filterlink.Null}, true},
		{Map{"input": map[string]any{"a": panicky{}}}, filterlink.Scalar{}, false},
		{nil, filterlink.Scalar{}, false},
		{"not a doc", filterlink.Scalar{}, false},
	} {
		if got, ok := path.Resolve(tc.doc); got != tc.want || ok != tc.ok {
			t.Errorf("Resolve(%v) = %v %v, want %v %v", tc.doc, got, ok, tc.want, tc.ok)
		}
	}
}
