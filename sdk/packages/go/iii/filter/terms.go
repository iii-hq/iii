package filter

import (
	"slices"
	"strconv"
	"strings"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/filterlink"
)

// The equality conditions of a filter's top-level $and, which package iii indexes the
// observers of a function by (decision 55): a document whose value at a condition's
// path equals none of its values does not match the filter, so an index of them by
// path and value rules such filters out with one lookup per path, and the whole filter
// is evaluated only on the others. They reach package iii through internal/filterlink.

func init() {
	filterlink.Terms = func(f any) []filterlink.Term {
		if flt, ok := f.(*Filter); ok && flt != nil {
			return flt.terms
		}
		return nil
	}
}

// scalarOf is v, a literal or a document value in JSON form (see view), as an index
// key, by the language's equality: numbers as doubles with -0 as 0, strings by their
// bytes, booleans and null as themselves. An array or an object equals no scalar.
func scalarOf(v any) (filterlink.Scalar, bool) {
	switch x := v.(type) {
	case nil:
		return filterlink.Scalar{Kind: filterlink.Null}, true
	case bool:
		s := filterlink.Scalar{Kind: filterlink.Bool}
		if x {
			s.Num = 1
		}
		return s, true
	case float64:
		if x == 0 {
			x = 0 // -0 equals 0
		}
		return filterlink.Scalar{Kind: filterlink.Number, Num: x}, true
	case string:
		return filterlink.Scalar{Kind: filterlink.String, Str: x}, true
	}
	return filterlink.Scalar{}, false
}

// appendTerms appends the equality conditions of a path clause of the top-level $and,
// whose operator object (validated) is ops: its $eq of a scalar, and its $in when every
// item is a scalar.
func appendTerms(terms []filterlink.Term, p *path, ops map[string]any) []filterlink.Term {
	var link *filterlink.Path
	linked := func() *filterlink.Path {
		if link == nil {
			link = p.link()
		}
		return link
	}
	if operand, ok := ops["$eq"]; ok {
		if s, ok := scalarOf(literal(operand)); ok {
			terms = append(terms, filterlink.Term{Path: linked(), Values: []filterlink.Scalar{s}})
		}
	}
	if items, ok := ops["$in"].([]any); ok && len(items) > 0 {
		values := make([]filterlink.Scalar, 0, len(items))
		for _, item := range items {
			s, ok := scalarOf(literal(item))
			if !ok {
				return terms // an array or object item: the clause is evaluated
			}
			if !slices.Contains(values, s) {
				values = append(values, s)
			}
		}
		terms = append(terms, filterlink.Term{Path: linked(), Values: values, In: true})
	}
	return terms
}

// link is the path as the index reads it: its key names the root and each segment's
// key, and Resolve reads it as a clause does.
func (p *path) link() *filterlink.Path {
	var key strings.Builder
	key.WriteString(string(p.root))
	for _, seg := range p.segments {
		key.WriteByte('/')
		key.WriteString(strconv.Itoa(len(seg.key)))
		key.WriteByte(':')
		key.WriteString(seg.key)
	}
	return &filterlink.Path{Key: key.String(), Resolve: p.resolveScalar}
}

// resolveScalar reads the path from doc, a Doc, as an index key. Like Matches it is
// total: a host value whose JSON encoding panics reads as absent, and a filter with a
// clause on that path does not match the document either.
func (p *path) resolveScalar(doc any) (s filterlink.Scalar, ok bool) {
	defer func() {
		if recover() != nil {
			s, ok = filterlink.Scalar{}, false
		}
	}()
	d := evalDoc{}
	d.doc, _ = doc.(Doc)
	v, present := p.resolve(&d, nil)
	if !present {
		return filterlink.Scalar{}, false
	}
	return scalarOf(v)
}
