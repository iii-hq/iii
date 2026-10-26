package filter

import (
	"strconv"
	"strings"
)

// segment is one path segment after the root. index is the array index a
// canonical decimal segment (0 or [1-9][0-9]*) selects, or -1; key is the
// object member it looks up either way.
type segment struct {
	key   string
	index int
}

type path struct {
	raw      string
	root     Root
	rootIdx  int
	segments []segment
}

// parsePath parses root ("." segment)*. "\." and "\\" escape inside keys; any
// other backslash sequence, an empty segment or an unknown root is
// INVALID_FILTER. "@" is a root only inside $any, where it is the only root
// allowed.
func parsePath(raw string, inAny bool) (*path, error) {
	var parts []string
	var current strings.Builder
	empty := func() error {
		if raw == "" {
			return invalid("empty path")
		}
		return invalid("path `%s`: empty segment", raw)
	}
	for i := 0; i < len(raw); i++ {
		c := raw[i]
		switch c {
		case '\\':
			if i+1 >= len(raw) {
				return nil, invalid("path `%s`: dangling `\\` at the end", raw)
			}
			next := raw[i+1]
			if next != '.' && next != '\\' {
				return nil, invalid("path `%s`: invalid escape `\\%c` (only `\\.` and `\\\\` are allowed)", raw, next)
			}
			current.WriteByte(next)
			i++
		case '.':
			if current.Len() == 0 {
				return nil, empty()
			}
			parts = append(parts, current.String())
			current.Reset()
		default:
			current.WriteByte(c)
		}
	}
	if current.Len() == 0 {
		return nil, empty()
	}
	parts = append(parts, current.String())

	first := Root(parts[0])
	known := first == rootAt || rootIndex(first) >= 0
	switch {
	case !known && inAny:
		return nil, invalid("path `%s`: inside `$any` paths start at `@`, the current element", raw)
	case !known:
		return nil, invalid("path `%s`: unknown root `%s` (expected input, output, error, caller or metadata)", raw, parts[0])
	case first == rootAt && !inAny:
		return nil, invalid("path `%s`: `@` paths are only allowed inside `$any`", raw)
	case first != rootAt && inAny:
		return nil, invalid("path `%s`: inside `$any` paths start at `@`, the current element", raw)
	}
	if len(parts)-1 > MaxPathSegments {
		return nil, invalid("path `%s`: %d segments after the root (limit %d)", raw, len(parts)-1, MaxPathSegments)
	}
	p := &path{raw: raw, root: first, rootIdx: rootIndex(first)}
	for _, key := range parts[1:] {
		p.segments = append(p.segments, segment{key: key, index: canonicalIndex(key)})
	}
	return p, nil
}

// canonicalIndex is the array index of a canonical decimal segment, or -1.
// "01", "-1" and numbers beyond int never select an element.
func canonicalIndex(s string) int {
	if s == "" || (s[0] == '0' && len(s) > 1) {
		return -1
	}
	for i := 0; i < len(s); i++ {
		if s[i] < '0' || s[i] > '9' {
			return -1
		}
	}
	n, err := strconv.Atoi(s)
	if err != nil {
		return -1
	}
	return n
}

// resolve reads the path from the document (or from el, the current $any
// element, for "@" paths). The value is in JSON form (see view); present is
// false when the path is absent.
func (p *path) resolve(d *evalDoc, el any) (any, bool) {
	var cur any
	var ok bool
	if p.root == rootAt {
		cur, ok = el, true
	} else {
		cur, ok = d.root(p.rootIdx)
	}
	for i := range p.segments {
		if !ok {
			return nil, false
		}
		seg := &p.segments[i]
		switch x := cur.(type) {
		case map[string]any:
			member, has := x[seg.key]
			if !has {
				return nil, false
			}
			cur, ok = view(member)
		case []any:
			if seg.index < 0 || seg.index >= len(x) {
				return nil, false
			}
			cur, ok = viewElement(x[seg.index]), true
		default:
			// strings, numbers, booleans and null have no children
			return nil, false
		}
	}
	return cur, ok
}

// evalDoc memoizes the JSON form of each root for one evaluation, so a host
// value read through encoding/json is converted once per Matches call.
type evalDoc struct {
	doc     Doc
	loaded  uint8
	present uint8
	vals    [len(rootOrder)]any
}

func (d *evalDoc) root(i int) (any, bool) {
	bit := uint8(1) << i
	if d.loaded&bit == 0 {
		d.loaded |= bit
		if d.doc != nil {
			if raw, ok := d.doc.Root(rootOrder[i]); ok {
				if v, ok := view(raw); ok {
					d.vals[i] = v
					d.present |= bit
				}
			}
		}
	}
	return d.vals[i], d.present&bit != 0
}
