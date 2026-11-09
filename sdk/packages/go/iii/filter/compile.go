package filter

// Validation (filters.md §2, §4) and compilation into closures, in one pass.
// Mirrors crates/iii-filter (parse.rs, eval.rs) and the Node SDK's
// src/filter/compile.ts: the same limits, the same error code and the same
// semantics. Keys are walked in sorted order, so the error reported for a
// filter with several problems does not depend on map iteration order.
//
// Limits, as the vectors define them:
//   - depth: the top-level filter is at depth 1; the filters of a $or, the
//     operand of $not and the operand of $any are one level deeper than the
//     object holding them; the filters of a $and stay at the same level;
//   - nodes: every operator ($eq ... $any) and every $or / $not counts one;
//     paths and $and are free;
//   - literal depth: arrays / objects nested in one $eq / $ne operand or one
//     $in / $nin item, at most 32;
//   - encoded size: the compact JSON of the filter as given, in UTF-8 bytes.

import (
	"encoding/json"
	"math"
	"strconv"
	"strings"

	"github.com/iii-hq/iii/sdk/packages/go/iii/internal/filterlink"
)

// pred is a compiled filter node; el is the current $any element ("@").
type pred func(d *evalDoc, el any) bool

// opFn is a compiled value operator; present is false when the path is absent.
type opFn func(v any, present bool, d *evalDoc) bool

var valueOperators = map[string]bool{
	"$eq": true, "$ne": true, "$in": true, "$nin": true,
	"$gt": true, "$gte": true, "$lt": true, "$lte": true,
	"$exists": true, "$prefix": true, "$any": true,
}

type compiler struct {
	nodes int
	roots Roots
}

func (c *compiler) take() error {
	c.nodes++
	if c.nodes > MaxNodes {
		return invalid("more than %d operator / clause nodes", MaxNodes)
	}
	return nil
}

func and(clauses []pred) pred {
	switch len(clauses) {
	case 0:
		return func(*evalDoc, any) bool { return true }
	case 1:
		return clauses[0]
	case 2:
		a, b := clauses[0], clauses[1]
		return func(d *evalDoc, el any) bool { return a(d, el) && b(d, el) }
	default:
		return func(d *evalDoc, el any) bool {
			for _, c := range clauses {
				if !c(d, el) {
					return false
				}
			}
			return true
		}
	}
}

func (c *compiler) parseFilter(src any, depth int, inAny bool) (pred, error) {
	var clauses []pred
	if err := c.collectClauses(src, depth, inAny, &clauses, nil); err != nil {
		return nil, err
	}
	return and(clauses), nil
}

// collectClauses collects the clauses of src and of every filter its $ands
// hold. $and adds no depth, so it is walked with an explicit stack. For the
// top-level filter, terms collects the equality conditions of its path clauses
// (appendTerms); it is nil below a $or, a $not or a $any.
func (c *compiler) collectClauses(src any, depth int, inAny bool, out *[]pred, terms *[]filterlink.Term) error {
	if depth > MaxDepth {
		return invalid("filters nested deeper than %d levels", MaxDepth)
	}
	pending := []any{src}
	for len(pending) > 0 {
		current := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		obj, ok := current.(map[string]any)
		if !ok {
			return invalid("a filter must be a JSON object, found %s", kind(current))
		}
		for _, key := range sortedKeys(obj) {
			value := obj[key]
			switch {
			case key == "$and":
				items, err := filterList("$and", value)
				if err != nil {
					return err
				}
				for i := len(items) - 1; i >= 0; i-- {
					pending = append(pending, items[i])
				}
			case key == "$or":
				if err := c.take(); err != nil {
					return err
				}
				items, err := filterList("$or", value)
				if err != nil {
					return err
				}
				children := make([]pred, 0, len(items))
				for _, item := range items {
					child, err := c.parseFilter(item, depth+1, inAny)
					if err != nil {
						return err
					}
					children = append(children, child)
				}
				*out = append(*out, func(d *evalDoc, el any) bool {
					for _, child := range children {
						if child(d, el) {
							return true
						}
					}
					return false
				})
			case key == "$not":
				if err := c.take(); err != nil {
					return err
				}
				if _, ok := value.(map[string]any); !ok {
					return invalid("`$not` takes a filter object, found %s", kind(value))
				}
				child, err := c.parseFilter(value, depth+1, inAny)
				if err != nil {
					return err
				}
				*out = append(*out, func(d *evalDoc, el any) bool { return !child(d, el) })
			case valueOperators[key]:
				return invalid("`%s` needs a path: write {\"<path>\": {\"%s\": ...}}", key, key)
			case strings.HasPrefix(key, "$"):
				return invalid("unknown operator `%s` (a filter holds paths, `$and`, `$or` and `$not`)", key)
			default:
				p, err := parsePath(key, inAny)
				if err != nil {
					return err
				}
				if p.root != rootAt {
					c.roots.add(p.root)
				}
				ops, err := c.parseOperatorObject(key, value, depth)
				if err != nil {
					return err
				}
				*out = append(*out, pathClause(p, ops))
				if terms != nil {
					*terms = appendTerms(*terms, p, value.(map[string]any))
				}
			}
		}
	}
	return nil
}

func pathClause(p *path, ops []opFn) pred {
	if len(ops) == 1 {
		op := ops[0]
		return func(d *evalDoc, el any) bool {
			v, present := p.resolve(d, el)
			return op(v, present, d)
		}
	}
	return func(d *evalDoc, el any) bool {
		v, present := p.resolve(d, el)
		for _, op := range ops {
			if !op(v, present, d) {
				return false
			}
		}
		return true
	}
}

func filterList(name string, value any) ([]any, error) {
	items, ok := value.([]any)
	if !ok {
		return nil, invalid("`%s` takes an array of filters, found %s", name, kind(value))
	}
	if len(items) == 0 {
		return nil, invalid("`%s` takes a non-empty array of filters", name)
	}
	return items, nil
}

func (c *compiler) parseOperatorObject(path string, value any, depth int) ([]opFn, error) {
	obj, ok := value.(map[string]any)
	if !ok {
		return nil, invalid("`%s`: expected an operator object such as {\"$eq\": ...}; literal values are not allowed", path)
	}
	if len(obj) == 0 {
		return nil, invalid("`%s`: empty operator object", path)
	}
	for k := range obj {
		if !strings.HasPrefix(k, "$") {
			return nil, invalid("`%s`: expected an operator object such as {\"$eq\": ...}; literal values are not allowed", path)
		}
	}
	return c.parseOperators(path, obj, depth)
}

// parseOperators compiles the operators of an operator object; depth is the
// level of the object holding them.
func (c *compiler) parseOperators(path string, obj map[string]any, depth int) ([]opFn, error) {
	ops := make([]opFn, 0, len(obj))
	for _, name := range sortedKeys(obj) {
		if err := c.take(); err != nil {
			return nil, err
		}
		op, err := c.parseOperator(path, name, obj[name], depth)
		if err != nil {
			return nil, err
		}
		ops = append(ops, op)
	}
	return ops, nil
}

func (c *compiler) parseOperator(path, name string, operand any, depth int) (opFn, error) {
	wrong := func(expected string) error {
		return invalid("`%s`: `%s` takes %s, found %s", path, name, expected, kind(operand))
	}
	switch name {
	case "$eq", "$ne":
		if err := checkLiteral(path, name, operand, 0); err != nil {
			return nil, err
		}
		lit := literal(operand)
		if name == "$eq" {
			return func(v any, present bool, _ *evalDoc) bool { return present && jsonEqual(v, lit) }, nil
		}
		return func(v any, present bool, _ *evalDoc) bool { return !present || !jsonEqual(v, lit) }, nil
	case "$in", "$nin":
		items, ok := operand.([]any)
		if !ok {
			return nil, wrong("an array")
		}
		if len(items) > MaxSetItems {
			return nil, invalid("`%s`: `%s` has %d items (limit %d)", path, name, len(items), MaxSetItems)
		}
		for _, item := range items {
			if err := checkLiteral(path, name, item, 0); err != nil {
				return nil, err
			}
		}
		set := literal(items).([]any)
		member := func(v any) bool {
			for _, item := range set {
				if jsonEqual(v, item) {
					return true
				}
			}
			return false
		}
		if name == "$in" {
			return func(v any, present bool, _ *evalDoc) bool { return present && member(v) }, nil
		}
		return func(v any, present bool, _ *evalDoc) bool { return !present || !member(v) }, nil
	case "$gt", "$gte", "$lt", "$lte":
		accept := func(cmp int) bool {
			switch name {
			case "$gt":
				return cmp > 0
			case "$gte":
				return cmp >= 0
			case "$lt":
				return cmp < 0
			default:
				return cmp <= 0
			}
		}
		switch bound := operand.(type) {
		case json.Number:
			if err := checkNumber(path, name, bound); err != nil {
				return nil, err
			}
			b := literal(bound).(float64)
			return func(v any, present bool, _ *evalDoc) bool {
				f, ok := v.(float64)
				if !present || !ok {
					return false
				}
				switch {
				case f < b:
					return accept(-1)
				case f > b:
					return accept(1)
				default:
					return accept(0)
				}
			}, nil
		case string:
			// Go compares strings byte by byte: UTF-8 byte order is code point order.
			return func(v any, present bool, _ *evalDoc) bool {
				s, ok := v.(string)
				return present && ok && accept(strings.Compare(s, bound))
			}, nil
		}
		return nil, wrong("a number or a string")
	case "$exists":
		want, ok := operand.(bool)
		if !ok {
			return nil, wrong("a boolean")
		}
		return func(_ any, present bool, _ *evalDoc) bool { return present == want }, nil
	case "$prefix":
		prefix, ok := operand.(string)
		if !ok {
			return nil, wrong("a string")
		}
		// Both strings are valid UTF-8, so a byte prefix ends on a code point boundary.
		return func(v any, present bool, _ *evalDoc) bool {
			s, ok := v.(string)
			return present && ok && strings.HasPrefix(s, prefix)
		}, nil
	case "$any":
		return c.parseAny(path, operand, depth)
	case "$and", "$or", "$not":
		return nil, invalid("`%s`: `%s` combines filters and is not a value operator", path, name)
	}
	return nil, invalid("`%s`: unknown operator `%s`", path, name)
}

// parseAny compiles $any: an operator object (every key a value operator)
// applied to each element, or a filter over the element ("@" paths, $and,
// $or, $not). {} is the empty filter: it matches any non-empty array.
func (c *compiler) parseAny(path string, operand any, depth int) (opFn, error) {
	obj, ok := operand.(map[string]any)
	if !ok {
		return nil, invalid("`%s`: `$any` takes an operator object or a filter over `@`, found %s", path, kind(operand))
	}
	if depth+1 > MaxDepth {
		return nil, invalid("filters nested deeper than %d levels", MaxDepth)
	}
	allOperators := len(obj) > 0
	for k := range obj {
		if !valueOperators[k] {
			allOperators = false
			break
		}
	}
	if allOperators {
		ops, err := c.parseOperators(path, obj, depth+1)
		if err != nil {
			return nil, err
		}
		return func(v any, present bool, d *evalDoc) bool {
			items, ok := v.([]any)
			if !present || !ok {
				return false
			}
		elements:
			for _, item := range items {
				el := viewElement(item)
				for _, op := range ops {
					if !op(el, true, d) {
						continue elements
					}
				}
				return true
			}
			return false
		}, nil
	}
	// "@" paths add no root: the $any clause's own path names it.
	saved := c.roots
	node, err := c.parseFilter(obj, depth+1, true)
	c.roots = saved
	if err != nil {
		return nil, err
	}
	return func(v any, present bool, d *evalDoc) bool {
		items, ok := v.([]any)
		if !present || !ok {
			return false
		}
		for _, item := range items {
			if node(d, viewElement(item)) {
				return true
			}
		}
		return false
	}, nil
}

// checkLiteral rejects numeric literals outside ±2^53 anywhere inside an operand,
// and operands nested deeper than MaxLiteralDepth arrays / objects. depth is the
// nesting of the containers holding value.
func checkLiteral(path, name string, value any, depth int) error {
	switch x := value.(type) {
	case json.Number:
		return checkNumber(path, name, x)
	case []any:
		if depth+1 > MaxLiteralDepth {
			return invalid("`%s`: `%s` nests more than %d arrays / objects", path, name, MaxLiteralDepth)
		}
		for _, item := range x {
			if err := checkLiteral(path, name, item, depth+1); err != nil {
				return err
			}
		}
	case map[string]any:
		if depth+1 > MaxLiteralDepth {
			return invalid("`%s`: `%s` nests more than %d arrays / objects", path, name, MaxLiteralDepth)
		}
		for _, k := range sortedKeys(x) {
			if err := checkLiteral(path, name, x[k], depth+1); err != nil {
				return err
			}
		}
	}
	return nil
}

// checkNumber enforces |n| <= 2^53. Integers are compared exactly; any other
// number by its double value (as crates/iii-filter does).
func checkNumber(path, name string, n json.Number) error {
	s := string(n)
	ok := false
	if !strings.ContainsAny(s, ".eE") {
		if i, err := strconv.ParseInt(s, 10, 64); err == nil {
			ok = i >= -MaxSafeLiteral && i <= MaxSafeLiteral
		}
		// an integer beyond int64 is beyond 2^53 too
	} else if f, err := strconv.ParseFloat(s, 64); err == nil {
		ok = math.Abs(f) <= MaxSafeLiteral
	}
	if !ok {
		return invalid("`%s`: `%s` holds the number %s, outside ±2^53", path, name, s)
	}
	return nil
}
