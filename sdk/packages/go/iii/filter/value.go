package filter

import (
	"encoding/json"
	"math"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"
)

// view maps a host value to its JSON form one step at a time, as if the
// document were encoded with encoding/json and decoded again (filters.md §3,
// "Host-language values"), without encoding JSON-shaped values:
//
//   - nil, bool, string (valid UTF-8), float64, map[string]any and []any are
//     returned as they are (members are mapped when a path steps into them);
//   - json.Number and every Go integer or float type become a float64; a
//     float32 is read as its JSON spelling; NaN and ±Inf read as null;
//   - json.RawMessage is decoded;
//   - anything else (structs, pointers, typed maps and slices, Marshalers) is
//     encoded with encoding/json and decoded.
//
// present is false when encoding/json cannot encode the value (a func, a
// channel): JSON leaves it out.
func view(v any) (value any, present bool) {
	switch x := v.(type) {
	// v itself where it is its own JSON form: converting x back to an interface
	// would allocate
	case nil:
		return nil, true
	case bool:
		return v, true
	case string:
		if utf8.ValidString(x) {
			return v, true
		}
		return viaJSON(x)
	case float64:
		if math.IsNaN(x) || math.IsInf(x, 0) {
			return nil, true
		}
		return v, true
	case json.Number:
		if !validNumber(string(x)) {
			return nil, false // not a JSON number: encoding/json refuses it
		}
		f, _ := strconv.ParseFloat(string(x), 64) // beyond float64: ±Inf, read as null
		return finite(f), true
	case map[string]any:
		if x == nil {
			return nil, true // encoding/json writes a nil map as null
		}
		return x, true
	case []any:
		if x == nil {
			return nil, true // and a nil slice
		}
		return x, true
	case int:
		return float64(x), true
	case int8:
		return float64(x), true
	case int16:
		return float64(x), true
	case int32:
		return float64(x), true
	case int64:
		return float64(x), true
	case uint:
		return float64(x), true
	case uint8:
		return float64(x), true
	case uint16:
		return float64(x), true
	case uint32:
		return float64(x), true
	case uint64:
		return float64(x), true
	case float32:
		f := float64(x)
		if math.IsNaN(f) || math.IsInf(f, 0) {
			return nil, true
		}
		parsed, _ := strconv.ParseFloat(strconv.FormatFloat(f, 'g', -1, 32), 64)
		return parsed, true
	case json.RawMessage:
		if x == nil {
			return nil, true // encoding/json writes a nil RawMessage as null
		}
		decoded, err := decodeJSON(x)
		if err != nil {
			return nil, false
		}
		return view(decoded)
	default:
		return viaJSON(v)
	}
}

// viewElement is an array element: a member JSON leaves out is null.
func viewElement(v any) any {
	out, ok := view(v)
	if !ok {
		return nil
	}
	return out
}

func viaJSON(v any) (any, bool) {
	data, err := json.Marshal(v)
	if err != nil {
		return nil, false
	}
	decoded, err := decodeJSON(data)
	if err != nil {
		return nil, false
	}
	return view(decoded)
}

func finite(f float64) any {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return nil
	}
	return f
}

// literal converts a validated operand (decoded with json.Number) to the
// form the evaluator compares against: numbers become float64.
func literal(v any) any {
	switch x := v.(type) {
	case json.Number:
		f, _ := strconv.ParseFloat(string(x), 64)
		return f
	case []any:
		out := make([]any, len(x))
		for i, item := range x {
			out[i] = literal(item)
		}
		return out
	case map[string]any:
		out := make(map[string]any, len(x))
		for k, item := range x {
			out[k] = literal(item)
		}
		return out
	default:
		return v
	}
}

// jsonEqual is JSON deep equality between a host value in JSON form and a
// literal. Numbers compare as doubles (1 == 1.0, -0 == 0), object key order
// is ignored, array order is not, and there is no coercion between types.
func jsonEqual(host, lit any) bool {
	switch l := lit.(type) {
	case nil:
		return host == nil
	case bool:
		h, ok := host.(bool)
		return ok && h == l
	case string:
		h, ok := host.(string)
		return ok && h == l
	case float64:
		h, ok := host.(float64)
		return ok && h == l
	case []any:
		h, ok := host.([]any)
		if !ok || len(h) != len(l) {
			return false
		}
		for i := range l {
			if !jsonEqual(viewElement(h[i]), l[i]) {
				return false
			}
		}
		return true
	case map[string]any:
		h, ok := host.(map[string]any)
		if !ok {
			return false
		}
		for k, want := range l {
			member, has := h[k]
			if !has {
				return false
			}
			got, present := view(member)
			if !present || !jsonEqual(got, want) {
				return false
			}
		}
		// the host must not hold more members than the literal
		present := 0
		for _, member := range h {
			if _, ok := view(member); ok {
				present++
				if present > len(l) {
					return false
				}
			}
		}
		return present == len(l)
	}
	return false
}

// fitsEncoded reports whether v's compact JSON encoding (no whitespace,
// non-ASCII not escaped, as JSON.stringify and serde_json write it) is at
// most limit bytes of UTF-8.
func fitsEncoded(v any, limit int) bool {
	n := 0
	return addEncoded(v, &n, limit)
}

func addEncoded(v any, n *int, limit int) bool {
	switch x := v.(type) {
	case nil:
		*n += 4
	case bool:
		if x {
			*n += 4
		} else {
			*n += 5
		}
	case json.Number:
		// as a canonical encoder writes the parsed value, not as spelled (1.000 is 1)
		*n += len(canonicalNumber(string(x)))
	case float64:
		*n += len(formatFloat(x))
	case string:
		*n += encodedStringLen(x)
	case []any:
		*n += 2
		for i, item := range x {
			if i > 0 {
				*n++
			}
			if !addEncoded(item, n, limit) {
				return false
			}
		}
	case map[string]any:
		*n += 2
		keys := sortedKeys(x)
		for i, k := range keys {
			if i > 0 {
				*n++
			}
			*n += encodedStringLen(k) + 1
			if !addEncoded(x[k], n, limit) {
				return false
			}
		}
	}
	return *n <= limit
}

func encodedStringLen(s string) int {
	n := 2
	for i := 0; i < len(s); {
		c := s[i]
		switch {
		case c == '"' || c == '\\' || c == '\b' || c == '\f' || c == '\n' || c == '\r' || c == '\t':
			n += 2
			i++
		case c < 0x20:
			n += 6
			i++
		case c < utf8.RuneSelf:
			n++
			i++
		default:
			r, size := utf8.DecodeRuneInString(s[i:])
			if r == utf8.RuneError && size == 1 {
				n += 3 // written as U+FFFD
			} else {
				n += size
			}
			i += size
		}
	}
	return n
}

// canonicalNumber is the shortest encoding of a JSON number's value: integers as
// their digits, other numbers as JavaScript (and encoding/json) write a double.
func canonicalNumber(s string) string {
	if !strings.ContainsAny(s, ".eE") {
		if i, err := strconv.ParseInt(s, 10, 64); err == nil {
			return strconv.FormatInt(i, 10)
		}
		if u, err := strconv.ParseUint(s, 10, 64); err == nil {
			return strconv.FormatUint(u, 10)
		}
	}
	f, err := strconv.ParseFloat(s, 64)
	if err != nil {
		return s
	}
	return formatFloat(f)
}

// formatFloat writes a double as JSON.stringify does: fixed notation for
// 1e-6 <= |f| < 1e21, otherwise an exponent without leading zeros.
func formatFloat(f float64) string {
	format := byte('f')
	if abs := math.Abs(f); abs != 0 && (abs < 1e-6 || abs >= 1e21) {
		format = 'e'
	}
	b := strconv.AppendFloat(nil, f, format, -1, 64)
	if format == 'e' {
		// e-07 -> e-7
		if n := len(b); n >= 4 && b[n-4] == 'e' && b[n-3] == '-' && b[n-2] == '0' {
			b[n-2] = b[n-1]
			b = b[:n-1]
		}
	}
	return string(b)
}

// validNumber reports whether s is a JSON number: -?(0|[1-9][0-9]*)(.[0-9]+)?([eE][+-]?[0-9]+)?
func validNumber(s string) bool {
	i := 0
	digits := func() int {
		start := i
		for i < len(s) && s[i] >= '0' && s[i] <= '9' {
			i++
		}
		return i - start
	}
	if i < len(s) && s[i] == '-' {
		i++
	}
	switch {
	case i < len(s) && s[i] == '0':
		i++
	case i < len(s) && s[i] >= '1' && s[i] <= '9':
		digits()
	default:
		return false
	}
	if i < len(s) && s[i] == '.' {
		i++
		if digits() == 0 {
			return false
		}
	}
	if i < len(s) && (s[i] == 'e' || s[i] == 'E') {
		i++
		if i < len(s) && (s[i] == '+' || s[i] == '-') {
			i++
		}
		if digits() == 0 {
			return false
		}
	}
	return i == len(s)
}

func sortedKeys(m map[string]any) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
