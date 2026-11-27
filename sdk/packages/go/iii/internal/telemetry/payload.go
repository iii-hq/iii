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

// sensitiveURLParams are the query parameters that carry a credential in a signed URL
// (S3 and R2 SigV2/SigV4, GCS, Azure SAS, CloudFront, the storage worker's local
// provider), lowercased. The engine and every SDK carry the same list.
var sensitiveURLParams = map[string]bool{
	"token":                true,
	"sig":                  true,
	"signature":            true,
	"policy":               true,
	"key-pair-id":          true,
	"awsaccesskeyid":       true,
	"x-amz-signature":      true,
	"x-amz-credential":     true,
	"x-amz-security-token": true,
	"x-goog-signature":     true,
	"x-goog-credential":    true,
}

func isSensitiveURLParam(name string) bool {
	return IsSensitiveKey(name) || sensitiveURLParams[strings.ToLower(name)]
}

// redactURLCredentials replaces the credentials in text with RedactedPlaceholder when it
// is a URL: one that starts with scheme:// or /. The password of its userinfo goes, or
// the whole userinfo when it has no ':' (a token used as the user name); the authority
// runs to the first '/', '?' or '#', the userinfo to the last '@' in it. So does the
// value of every query parameter whose name is sensitive (the query runs from the first
// '?' to the next '#'). Everything else is kept as it is; empty and
// already redacted values are skipped.
func redactURLCredentials(text string) string {
	authorityStart := -1
	if !strings.HasPrefix(text, "/") {
		authorityStart = schemePrefixLen(text)
		if authorityStart == 0 {
			return text
		}
	}
	var secrets [][2]int
	secret := func(start, end int) {
		if value := text[start:end]; value != "" && value != RedactedPlaceholder {
			secrets = append(secrets, [2]int{start, end})
		}
	}
	if authorityStart > 0 {
		authorityEnd := len(text)
		if i := strings.IndexAny(text[authorityStart:], "/?#"); i >= 0 {
			authorityEnd = authorityStart + i
		}
		if at := strings.LastIndexByte(text[authorityStart:authorityEnd], '@'); at >= 0 {
			if colon := strings.IndexByte(text[authorityStart:authorityStart+at], ':'); colon >= 0 {
				secret(authorityStart+colon+1, authorityStart+at)
			} else {
				secret(authorityStart, authorityStart+at)
			}
		}
	}
	if question := strings.IndexByte(text, '?'); question >= 0 {
		queryEnd := len(text)
		if i := strings.IndexByte(text[question+1:], '#'); i >= 0 {
			queryEnd = question + 1 + i
		}
		paramStart := question + 1
		for _, param := range strings.Split(text[question+1:queryEnd], "&") {
			if name, _, ok := strings.Cut(param, "="); ok && isSensitiveURLParam(name) {
				secret(paramStart+len(name)+1, paramStart+len(param))
			}
			paramStart += len(param) + 1
		}
	}
	if len(secrets) == 0 {
		return text
	}
	var b strings.Builder
	b.Grow(len(text))
	copied := 0
	for _, s := range secrets {
		b.WriteString(text[copied:s[0]])
		b.WriteString(RedactedPlaceholder)
		copied = s[1]
	}
	b.WriteString(text[copied:])
	return b.String()
}

// schemePrefixLen returns the length of the "scheme://" that starts text (a letter, then
// letters, digits, '+', '-' or '.'), or 0 when it does not start with one.
func schemePrefixLen(text string) int {
	isLetter := func(c byte) bool { return 'a' <= c && c <= 'z' || 'A' <= c && c <= 'Z' }
	if text == "" || !isLetter(text[0]) {
		return 0
	}
	i := 1
	for i < len(text) {
		c := text[i]
		if !isLetter(c) && !('0' <= c && c <= '9') && c != '+' && c != '-' && c != '.' {
			break
		}
		i++
	}
	if !strings.HasPrefix(text[i:], "://") {
		return 0
	}
	return i + 3
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
// (Node redact), and the credentials in string values that are URLs too: the password
// of the userinfo (the whole userinfo when it has no password) and the values of query
// parameters with a sensitive name, such as the signature of a presigned URL. Maps of type map[string]any and slices of type []any
// are copied recursively and nil and other scalars are returned as they are; any other
// value (a struct, a typed map, json.RawMessage) is serialized to JSON and returned
// redacted as a json.RawMessage, keeping its field order.
func Redact(v any) any {
	switch x := v.(type) {
	case string:
		return redactURLCredentials(x)
	case nil, bool, int, int8, int16, int32, int64, uint, uint8, uint16, uint32, uint64, float32, float64, json.Number:
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
// RedactedPlaceholder, at any depth, and the credentials in string values that are URLs
// too (Redact), keeping key order and number literals.
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
		writeJSONString(buf, redactURLCredentials(t))
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
