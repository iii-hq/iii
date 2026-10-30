package telemetry

import (
	"encoding/json"
	"strings"
	"testing"
	"unicode/utf8"
)

func redactedMap(t *testing.T, v any) map[string]any {
	t.Helper()
	s, _ := RedactAndTruncate(v, 0)
	var out map[string]any
	if err := json.Unmarshal([]byte(s), &out); err != nil {
		t.Fatalf("%s: %v", s, err)
	}
	return out
}

// The cases of the Rust SDK's payload tests (payload.rs), which mirror Node payload.ts.
func TestRedactSensitiveKeys(t *testing.T) {
	out := redactedMap(t, map[string]any{"api_key": "sk-1", "model": "claude-3-5"})
	if out["api_key"] != RedactedPlaceholder || out["model"] != "claude-3-5" {
		t.Errorf("top level: %v", out)
	}
	out = redactedMap(t, map[string]any{
		"headers": map[string]any{"Authorization": "Bearer x", "Content-Type": "application/json"},
		"config":  map[string]any{"secret": "s"},
	})
	h := out["headers"].(map[string]any)
	if h["Authorization"] != RedactedPlaceholder || h["Content-Type"] != "application/json" || out["config"].(map[string]any)["secret"] != RedactedPlaceholder {
		t.Errorf("nested: %v", out)
	}
	out = redactedMap(t, map[string]any{"accounts": []any{
		map[string]any{"access_token": "a", "user": "alice"},
		map[string]any{"access_token": "b", "user": "bob"},
	}})
	accounts := out["accounts"].([]any)
	if accounts[0].(map[string]any)["access_token"] != RedactedPlaceholder || accounts[1].(map[string]any)["user"] != "bob" {
		t.Errorf("arrays: %v", out)
	}
	out = redactedMap(t, map[string]any{"credentials": map[string]any{"user": "u", "pass": "p"}})
	if out["credentials"] != RedactedPlaceholder {
		t.Errorf("a sensitive key redacts a whole object: %v", out)
	}
	out = redactedMap(t, map[string]any{"API_KEY": 1, "PassWord": 2, "client_SECRET": 3})
	for _, k := range []string{"API_KEY", "PassWord", "client_SECRET"} {
		if out[k] != RedactedPlaceholder {
			t.Errorf("case-insensitive %s: %v", k, out)
		}
	}
	out = redactedMap(t, map[string]any{"token": "t", "id_token": "i", "x-token": "x", "notification": "ping", "function_id": "do_thing"})
	if out["token"] != RedactedPlaceholder || out["id_token"] != RedactedPlaceholder || out["x-token"] != RedactedPlaceholder ||
		out["notification"] != "ping" || out["function_id"] != "do_thing" {
		t.Errorf("token rules: %v", out)
	}
}

// Raw JSON and structs keep their field order and number literals, as JSON.stringify of
// the parsed object keeps insertion order.
func TestRedactKeepsOrder(t *testing.T) {
	got, _ := RedactAndTruncate(json.RawMessage(`{"b":1.50,"a":{"password":"x","z":[1,{"secret":2}]},"c":"<&>"}`), 0)
	if want := `{"b":1.50,"a":{"password":"[REDACTED]","z":[1,{"secret":"[REDACTED]"}]},"c":"<&>"}`; got != want {
		t.Errorf("got %s\nwant %s", got, want)
	}
	type login struct {
		User     string `json:"user"`
		Password string `json:"password"`
	}
	if got := Redact(login{User: "u", Password: "p"}); string(got.(json.RawMessage)) != `{"user":"u","password":"[REDACTED]"}` {
		t.Errorf("struct: %s", got)
	}
	if got := Redact([]any{map[string]any{"apikey": 1}, 2}).([]any); got[0].(map[string]any)["apikey"] != RedactedPlaceholder || got[1] != 2 {
		t.Errorf("slice: %v", got)
	}
	if got, _ := RedactAndTruncate(nil, 0); got != "null" {
		t.Errorf("nil -> %s", got)
	}
	if got, _ := RedactAndTruncate(make(chan int), 0); got != "null" {
		t.Errorf("unserializable -> %s", got)
	}
}

func TestTruncate(t *testing.T) {
	big := map[string]any{"data": strings.Repeat("x", 10000)}
	if out, truncated := RedactAndTruncate(big, 0); truncated || strings.HasSuffix(out, truncationMarker) {
		t.Error("no cap truncated")
	}
	out, truncated := RedactAndTruncate(big, 4096)
	if !truncated || !strings.HasSuffix(out, truncationMarker) || len(out) > 4096 {
		t.Errorf("cap 4096: truncated=%v len=%d", truncated, len(out))
	}
	for max := 1; max <= 40; max++ {
		out, truncated := RedactAndTruncate(big, max)
		if !truncated || len(out) > max {
			t.Errorf("max=%d: truncated=%v len=%d %q", max, truncated, len(out), out)
		}
	}
	if out, _ := Truncate(strings.Repeat("x", 100), 5); out != `..."[` {
		t.Errorf("cap below the marker: %q", out)
	}
	if out, truncated := RedactAndTruncate(map[string]any{"data": strings.Repeat("y", 1_000_000)}, 0); truncated || len(out) < 1_000_000 {
		t.Error("unlimited large payload truncated")
	}
	out, _ = RedactAndTruncate(map[string]any{"s": strings.Repeat("é", 100)}, 51)
	if !utf8.ValidString(out) {
		t.Errorf("split a UTF-8 sequence: %q", out)
	}
	out, _ = RedactAndTruncate(map[string]any{"api_key": "sk-must-not-leak", "data": strings.Repeat("z", 5000)}, 100)
	if strings.Contains(out, "sk-must-not-leak") || !strings.Contains(out, RedactedPlaceholder) {
		t.Errorf("redaction before truncation: %q", out)
	}
}

func TestResolveMaxBytes(t *testing.T) {
	for _, tc := range []struct {
		value string
		set   bool
		want  int
	}{
		{set: false, want: 0},
		{value: "", set: true, want: 0},
		{value: "unlimited", set: true, want: 0},
		{value: "UNLIMITED", set: true, want: 0},
		{value: "8192mb", set: true, want: 0},
		{value: "-1", set: true, want: 0},
		{value: "0", set: true, want: 0},
		{value: " 4096 ", set: true, want: 4096},
	} {
		m := map[string]string{}
		if tc.set {
			m["III_TRACE_PAYLOAD_MAX_BYTES"] = tc.value
		}
		if got := resolveMaxBytes(envMap(m)); got != tc.want {
			t.Errorf("III_TRACE_PAYLOAD_MAX_BYTES=%q -> %d, want %d", tc.value, got, tc.want)
		}
	}
}

// Node: III_DISABLE_TRACE_PAYLOADS === '1' || value.toLowerCase() === 'true'.
func TestTracePayloadsEnabled(t *testing.T) {
	for value, want := range map[string]bool{"": true, "1": false, "true": false, "TRUE": false, "0": true, "yes": true} {
		t.Setenv("III_DISABLE_TRACE_PAYLOADS", value)
		if got := TracePayloadsEnabled(); got != want {
			t.Errorf("III_DISABLE_TRACE_PAYLOADS=%q -> %v, want %v", value, got, want)
		}
	}
}

func TestSafeStringify(t *testing.T) {
	type node struct{ Next *node }
	n := &node{}
	n.Next = n
	if got := SafeStringify(n); got != "[unserializable]" {
		t.Errorf("cyclic -> %q", got)
	}
	if got := SafeStringify(map[string]int{"a": 1}); got != `{"a":1}` {
		t.Errorf("map -> %q", got)
	}
}
