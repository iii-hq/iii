package telemetry

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"os"
	"strconv"
	"strings"
)

// RedactedPlaceholder replaces the value of a sensitive key (Node REDACTED_PLACEHOLDER).
const RedactedPlaceholder = "[REDACTED]"

// truncationMarker ends a truncated payload (Node TRUNCATION_MARKER).
const truncationMarker = `..."[TRUNCATED]"`

// sensitiveFragments is Node SENSITIVE_FRAGMENTS: a key containing any of them (any
// case) is redacted.
var sensitiveFragments = []string{
	"api_key",
	"apikey",
	"api-key",
	"password",
	"secret",
	"credential",
	"authorization",
	"auth_token",
	"access_token",
	"refresh_token",
	"bearer",
	"private_key",
	"client_secret",
}

// IsSensitiveKey is Node isSensitiveKey. "token" alone is too common a substring, so it
// must be the whole key or a _token / -token suffix.
func IsSensitiveKey(key string) bool {
	lower := strings.ToLower(key)
	for _, f := range sensitiveFragments {
		if strings.Contains(lower, f) {
			return true
		}
	}
	return lower == "token" || strings.HasSuffix(lower, "_token") || strings.HasSuffix(lower, "-token")
}

// TracePayloadsEnabled reports whether invocation payloads are recorded as span events:
// false when III_DISABLE_TRACE_PAYLOADS is "1" or "true" (any case), like the Node SDK.
// It is read on every invocation.
func TracePayloadsEnabled() bool {
	v := os.Getenv("III_DISABLE_TRACE_PAYLOADS")
	return !(v == "1" || strings.ToLower(v) == "true")
}

// ResolveMaxBytesFromEnv returns the payload cap from III_TRACE_PAYLOAD_MAX_BYTES, or 0
// (no cap) when it is unset, blank, "unlimited", not all digits or not positive (Node
// resolveMaxBytesFromEnv, whose null is 0 here).
func ResolveMaxBytesFromEnv() int {
	return resolveMaxBytes(os.LookupEnv)
}

func resolveMaxBytes(env LookupEnv) int {
	raw, ok := env("III_TRACE_PAYLOAD_MAX_BYTES")
	if !ok {
		return 0
	}
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" || strings.EqualFold(trimmed, "unlimited") {
		return 0
	}
	for _, r := range trimmed {
		if r < '0' || r > '9' {
			return 0
		}
	}
	n, err := strconv.Atoi(trimmed)
	if err != nil || n <= 0 {
		return 0
	}
	return n
}

// Redact returns v with the values of sensitive keys replaced by RedactedPlaceholder
// (Node redact). Maps of type map[string]any and slices of type []any are copied
// recursively and nil and scalars are returned as they are; any other value (a struct,
// a typed map, json.RawMessage) is serialized to JSON and returned redacted as a
// json.RawMessage, keeping its field order.
func Redact(v any) any {
	switch x := v.(type) {
	case nil, string, bool, int, int8, int16, int32, int64, uint, uint8, uint16, uint32, uint64, float32, float64, json.Number:
		return x
	case map[string]any:
		out := make(map[string]any, len(x))
		for k, e := range x {
			if IsSensitiveKey(k) {
				out[k] = RedactedPlaceholder
			} else {
				out[k] = Redact(e)
			}
		}
		return out
	case []any:
		out := make([]any, len(x))
		for i, e := range x {
			out[i] = Redact(e)
		}
		return out
	default:
		raw, err := marshalJSON(v)
		if err != nil {
			return nil
		}
		redacted, err := RedactJSON(raw)
		if err != nil {
			return nil
		}
		return json.RawMessage(redacted)
	}
}

// RedactJSON rewrites a JSON document with the values of sensitive keys replaced by
// RedactedPlaceholder, at any depth, keeping key order and number literals.
func RedactJSON(raw []byte) ([]byte, error) {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var buf bytes.Buffer
	if err := writeRedacted(dec, &buf); err != nil {
		return nil, err
	}
	if _, err := dec.Token(); err != io.EOF {
		return nil, errors.New("telemetry: trailing data after JSON value")
	}
	return buf.Bytes(), nil
}

func writeRedacted(dec *json.Decoder, buf *bytes.Buffer) error {
	tok, err := dec.Token()
	if err != nil {
		return err
	}
	switch t := tok.(type) {
	case json.Delim:
		switch t {
		case '{':
			buf.WriteByte('{')
			for first := true; dec.More(); first = false {
				keyTok, err := dec.Token()
				if err != nil {
					return err
				}
				key, _ := keyTok.(string)
				if !first {
					buf.WriteByte(',')
				}
				writeJSONString(buf, key)
				buf.WriteByte(':')
				if IsSensitiveKey(key) {
					var skip json.RawMessage
					if err := dec.Decode(&skip); err != nil {
						return err
					}
					writeJSONString(buf, RedactedPlaceholder)
					continue
				}
				if err := writeRedacted(dec, buf); err != nil {
					return err
				}
			}
			if _, err := dec.Token(); err != nil {
				return err
			}
			buf.WriteByte('}')
		case '[':
			buf.WriteByte('[')
			for first := true; dec.More(); first = false {
				if !first {
					buf.WriteByte(',')
				}
				if err := writeRedacted(dec, buf); err != nil {
					return err
				}
			}
			if _, err := dec.Token(); err != nil {
				return err
			}
			buf.WriteByte(']')
		default:
			return errors.New("telemetry: unexpected JSON delimiter")
		}
	case string:
		writeJSONString(buf, t)
	case json.Number:
		buf.WriteString(t.String())
	case bool:
		if t {
			buf.WriteString("true")
		} else {
			buf.WriteString("false")
		}
	case nil:
		buf.WriteString("null")
	}
	return nil
}

func writeJSONString(buf *bytes.Buffer, s string) {
	encoded, _ := marshalJSON(s)
	buf.Write(encoded)
}

// RedactAndTruncate redacts v, serializes it to JSON and caps the result at maxBytes
// (Node redactAndTruncate; maxBytes <= 0 means no cap). A value that cannot be
// serialized becomes "null".
func RedactAndTruncate(v any, maxBytes int) (string, bool) {
	var serialized []byte
	raw, err := marshalJSON(v)
	if err == nil {
		serialized, err = RedactJSON(raw)
	}
	if err != nil {
		serialized = []byte("null")
	}
	return Truncate(string(serialized), maxBytes)
}

// Truncate caps serialized at maxBytes bytes, ending it with the truncation marker and
// never splitting a UTF-8 sequence; when maxBytes is smaller than the marker only a
// prefix of the marker is returned (Node redactAndTruncate's truncation step).
func Truncate(serialized string, maxBytes int) (string, bool) {
	if maxBytes <= 0 || len(serialized) <= maxBytes {
		return serialized, false
	}
	if maxBytes <= len(truncationMarker) {
		return truncationMarker[:maxBytes], true
	}
	cut := maxBytes - len(truncationMarker)
	if cut > len(serialized) {
		cut = len(serialized)
	}
	for cut > 0 && cut < len(serialized) && serialized[cut]&0xc0 == 0x80 {
		cut--
	}
	return serialized[:cut] + truncationMarker, true
}

// SafeStringify serializes v to JSON, or returns "[unserializable]" when it cannot be
// (Node safeStringify; a cyclic value is reported as unserializable).
func SafeStringify(v any) string {
	b, err := marshalJSON(v)
	if err != nil {
		return "[unserializable]"
	}
	return string(b)
}
