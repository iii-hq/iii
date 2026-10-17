// Package filter is the observer filter language (iii-filter v1,
// tech-specs/2026-10-13-observe/filters.md): validation and evaluation with the
// same semantics as the engine's Rust evaluator (crates/iii-filter) and the
// other SDKs. The shared conformance vectors in sdk/fixtures/filters pin them
// down; the rules the spec leaves open are listed in that directory's README.
//
// A filter is a JSON object of path clauses with explicit operators, plus
// $and, $or and $not:
//
//	f, err := filter.Compile(map[string]any{"input.amount": map[string]any{"$gte": 500}})
//	f.Matches(filter.Map{"input": map[string]any{"amount": 750}}) // true
//
// Documents are read as JSON would write them: a host value that is not
// already a JSON-shaped Go value (map[string]any, []any, string, float64,
// json.Number, bool, nil) is read through encoding/json, and every number
// compares as an IEEE-754 double. Evaluation is total: a compiled filter never
// panics and answers true or false for every document.
package filter

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"strconv"
	"unicode/utf8"
)

// Structural limits (filters.md §4; the fixtures README "Limits" defines how they count).
const (
	// MaxDepth is the deepest a filter may nest; the top-level filter is at depth 1.
	MaxDepth = 8
	// MaxNodes bounds the operator and $or / $not nodes of one filter.
	MaxNodes = 64
	// MaxSetItems bounds the items of one $in / $nin array.
	MaxSetItems = 256
	// MaxPathSegments bounds the segments after a path's root.
	MaxPathSegments = 16
	// MaxEncodedBytes bounds the UTF-8 size of the filter's compact JSON encoding.
	MaxEncodedBytes = 8192
	// MaxSafeLiteral bounds numeric literals: |n| <= 2^53.
	MaxSafeLiteral = 1 << 53
	// MaxLiteralDepth bounds the arrays / objects nested in one $eq / $ne operand or
	// one $in / $nin item (a scalar is 0, [] is 1).
	MaxLiteralDepth = 32
	// MaxJSONDepth bounds the arrays / objects nested in the whole filter value, $and
	// wrappers and literals included ({} is 1).
	MaxJSONDepth = 120
)

// ErrorCode is the protocol error code of a rejected filter (protocol.md §4).
type ErrorCode string

const (
	// CodeInvalidFilter is a filter the language rejects.
	CodeInvalidFilter ErrorCode = "INVALID_FILTER"
	// CodeInvalidFilterPath is a path a closed schema does not have. Only the
	// engine checks schemas; this package never returns it.
	CodeInvalidFilterPath ErrorCode = "INVALID_FILTER_PATH"
)

// Error is a filter the language rejects. Recover it with errors.As.
type Error struct {
	Code    ErrorCode
	Message string
}

func (e *Error) Error() string { return string(e.Code) + ": " + e.Message }

func invalid(format string, args ...any) *Error {
	return &Error{Code: CodeInvalidFilter, Message: fmt.Sprintf(format, args...)}
}

// Root is a path's first segment: the part of the document it reads.
type Root string

const (
	RootInput    Root = "input"
	RootOutput   Root = "output"
	RootError    Root = "error"
	RootCaller   Root = "caller"
	RootMetadata Root = "metadata"
)

// rootAt is the current $any element; it is never a document root.
const rootAt Root = "@"

var rootOrder = [...]Root{RootInput, RootOutput, RootError, RootCaller, RootMetadata}

func rootIndex(r Root) int {
	for i, x := range rootOrder {
		if x == r {
			return i
		}
	}
	return -1
}

// Roots tells which document roots a filter's clauses read.
type Roots struct {
	Input, Output, Error, Caller, Metadata bool
}

// Has reports whether the filter reads root.
func (r Roots) Has(root Root) bool {
	switch root {
	case RootInput:
		return r.Input
	case RootOutput:
		return r.Output
	case RootError:
		return r.Error
	case RootCaller:
		return r.Caller
	case RootMetadata:
		return r.Metadata
	}
	return false
}

func (r *Roots) add(root Root) {
	switch root {
	case RootInput:
		r.Input = true
	case RootOutput:
		r.Output = true
	case RootError:
		r.Error = true
	case RootCaller:
		r.Caller = true
	case RootMetadata:
		r.Metadata = true
	}
}

// Doc is the document a filter reads (filters.md §1): the value of each root,
// and whether it is present. A missing root is absent; a present nil is JSON null.
type Doc interface {
	Root(root Root) (value any, present bool)
}

// Map is a Doc backed by a map keyed by root name: a missing key is an absent
// root, a nil value is JSON null. Keys other than the five roots are ignored.
type Map map[string]any

// Root implements Doc.
func (m Map) Root(root Root) (any, bool) {
	v, ok := m[string(root)]
	return v, ok
}

// Filter is a validated filter, compiled once into a closure tree. It is
// immutable and safe for concurrent use.
type Filter struct {
	source   map[string]any
	pred     pred
	roots    Roots
	matchAll bool
}

var matchAll = &Filter{source: map[string]any{}, pred: func(*evalDoc, any) bool { return true }, matchAll: true}

// MatchAll returns the filter that matches every document ({}).
func MatchAll() *Filter { return matchAll }

// IsMatchAll reports whether the filter matches every document.
func (f *Filter) IsMatchAll() bool { return f.matchAll }

// Roots reports which roots the filter's clauses read.
func (f *Filter) Roots() Roots { return f.roots }

// Source returns a copy of the filter as decoded JSON (numbers as json.Number).
func (f *Filter) Source() map[string]any { return deepCopy(f.source).(map[string]any) }

func deepCopy(v any) any {
	switch x := v.(type) {
	case map[string]any:
		out := make(map[string]any, len(x))
		for k, item := range x {
			out[k] = deepCopy(item)
		}
		return out
	case []any:
		out := make([]any, len(x))
		for i, item := range x {
			out[i] = deepCopy(item)
		}
		return out
	}
	return v
}

// Matches evaluates the filter on doc. A nil doc has every root absent. It is
// total: a host value whose JSON encoding panics reads as no match.
func (f *Filter) Matches(doc Doc) (matched bool) {
	if f.matchAll {
		return true
	}
	defer func() {
		if recover() != nil {
			matched = false
		}
	}()
	return f.matches(doc)
}

// matches is Matches without the recover (the fuzz target calls it, so a panic of
// the evaluator itself is not hidden).
func (f *Filter) matches(doc Doc) bool {
	d := evalDoc{doc: doc}
	return f.pred(&d, nil)
}

// Parse validates and compiles a filter given as JSON text. It returns an
// *Error with code INVALID_FILTER when the filter is not valid.
func Parse(data []byte) (*Filter, error) {
	return parse(data, true)
}

// ParseNormalized compiles a filter in the normal form the engine pushes
// (functionregistered, observeradded). Every rule applies except the encoded
// size limit, which the engine checked on the filter as written (a normal form
// can be longer).
func ParseNormalized(data []byte) (*Filter, error) {
	return parse(data, false)
}

func parse(data []byte, checkSize bool) (*Filter, error) {
	if err := checkText(data); err != nil {
		return nil, err
	}
	v, err := decodeJSON(data)
	if err != nil {
		return nil, invalid("a filter must be JSON: %v", err)
	}
	return compileValue(v, checkSize)
}

// checkText rejects what encoding/json would silently repair, invalid UTF-8 and lone
// UTF-16 surrogate escapes ("\ud83d", which have no code point), and a value nested
// deeper than MaxJSONDepth, in one pass over the text, without recursion.
func checkText(data []byte) error {
	if !utf8.Valid(data) {
		return invalid("a filter must be valid UTF-8")
	}
	inString := false
	pendingHigh := false // the previous escape was a high surrogate
	depth := 0
	for i := 0; i < len(data); i++ {
		c := data[i]
		if !inString {
			switch c {
			case '"':
				inString = true
			case '{', '[':
				depth++
				if depth > MaxJSONDepth {
					return invalid("the filter nests more than %d arrays / objects", MaxJSONDepth)
				}
			case '}', ']':
				depth--
			}
			continue
		}
		switch c {
		case '"':
			if pendingHigh {
				return invalid("a filter string holds a lone UTF-16 surrogate")
			}
			inString = false
		case '\\':
			if i+1 >= len(data) {
				return nil // the decoder reports it
			}
			if data[i+1] != 'u' || i+5 >= len(data) {
				if pendingHigh {
					return invalid("a filter string holds a lone UTF-16 surrogate")
				}
				i++
				continue
			}
			n, err := strconv.ParseUint(string(data[i+2:i+6]), 16, 16)
			if err != nil {
				return nil // the decoder reports it
			}
			switch {
			case n >= 0xD800 && n <= 0xDBFF:
				if pendingHigh {
					return invalid("a filter string holds a lone UTF-16 surrogate")
				}
				pendingHigh = true
			case n >= 0xDC00 && n <= 0xDFFF:
				if !pendingHigh {
					return invalid("a filter string holds a lone UTF-16 surrogate")
				}
				pendingHigh = false
			default:
				if pendingHigh {
					return invalid("a filter string holds a lone UTF-16 surrogate")
				}
			}
			i += 5
		default:
			if pendingHigh {
				return invalid("a filter string holds a lone UTF-16 surrogate")
			}
		}
	}
	return nil
}

// Compile validates and compiles a filter given as a Go value. A
// json.RawMessage or []byte is read as JSON text; any other value is read
// through encoding/json, so what is validated is exactly what travels on the
// wire. It returns an *Error with code INVALID_FILTER when the filter is not
// valid.
func Compile(src any) (*Filter, error) {
	switch s := src.(type) {
	case json.RawMessage:
		return Parse(s)
	case []byte:
		return Parse(s)
	case nil:
		return nil, invalid("a filter must be a JSON object, found null")
	}
	data, err := json.Marshal(src)
	if err != nil {
		return nil, invalid("a filter must be JSON: %v", err)
	}
	return Parse(data)
}

func compileValue(v any, checkSize bool) (*Filter, error) {
	obj, ok := v.(map[string]any)
	if !ok {
		return nil, invalid("a filter must be a JSON object, found %s", kind(v))
	}
	if checkSize && !fitsEncoded(obj, MaxEncodedBytes) {
		return nil, invalid("the filter encodes to more than %d bytes", MaxEncodedBytes)
	}
	c := compiler{}
	var clauses []pred
	if err := c.collectClauses(obj, 1, false, &clauses); err != nil {
		return nil, err
	}
	if len(clauses) == 0 {
		return &Filter{source: obj, pred: matchAll.pred, matchAll: true}, nil
	}
	return &Filter{source: obj, pred: and(clauses), roots: c.roots}, nil
}

// decodeJSON reads exactly one JSON value, keeping numbers as json.Number.
func decodeJSON(data []byte) (any, error) {
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.UseNumber()
	var v any
	if err := dec.Decode(&v); err != nil {
		return nil, err
	}
	if _, err := dec.Token(); err != io.EOF {
		return nil, fmt.Errorf("trailing data after the JSON value")
	}
	return v, nil
}

func kind(v any) string {
	switch v.(type) {
	case nil:
		return "null"
	case bool:
		return "a boolean"
	case json.Number, float64:
		return "a number"
	case string:
		return "a string"
	case []any:
		return "an array"
	default:
		return "an object"
	}
}
