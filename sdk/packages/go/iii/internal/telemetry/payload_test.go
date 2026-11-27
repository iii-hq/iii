package telemetry

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
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

// urlVectorsPath is sdk/fixtures/redaction/url-credentials.json, relative to this
// package: the vectors the engine and every SDK run.
var urlVectorsPath = filepath.Join("..", "..", "..", "..", "..", "fixtures", "redaction", "url-credentials.json")

func wrapURL(value string) map[string]any {
	return map[string]any{"headers": map[string]any{"location": value}, "links": []any{value}}
}

func TestRedactURLCredentialsVectors(t *testing.T) {
	raw, err := os.ReadFile(urlVectorsPath)
	if err != nil {
		t.Fatal(err)
	}
	var vectors struct {
		Cases []struct {
			Note   string `json:"note"`
			Value  string `json:"value"`
			Expect string `json:"expect"`
		} `json:"cases"`
	}
	if err := json.Unmarshal(raw, &vectors); err != nil {
		t.Fatal(err)
	}
	if len(vectors.Cases) <= 10 {
		t.Fatalf("found %d cases in %s, expected the full set", len(vectors.Cases), urlVectorsPath)
	}
	for _, c := range vectors.Cases {
		t.Run(c.Note, func(t *testing.T) {
			want := wrapURL(c.Expect)
			if got := Redact(wrapURL(c.Value)); !reflect.DeepEqual(got, want) {
				t.Errorf("Redact = %v, want %v", got, want)
			}
			// RedactAndTruncate goes through RedactJSON.
			if got := redactedMap(t, wrapURL(c.Value)); !reflect.DeepEqual(got, want) {
				t.Errorf("RedactAndTruncate = %v, want %v", got, want)
			}
			if got := Redact(want); !reflect.DeepEqual(got, want) {
				t.Errorf("not idempotent: %v", got)
			}
		})
	}
}

func TestRedactURLCredentials(t *testing.T) {
	response := map[string]any{
		"status_code": 302,
		"headers": map[string]any{
			"location": "https://acct.r2.cloudflarestorage.com/bucket/key?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AK%2F20261126&X-Amz-Signature=abc",
		},
		"body": map[string]any{"upload": map[string]any{"url": "http://localhost:49200/v1/objects/upload?token=eyJ.a.b"}},
	}
	got, _ := RedactAndTruncate(response, 0)
	want := `{"body":{"upload":{"url":"http://localhost:49200/v1/objects/upload?token=[REDACTED]"}},"headers":{"location":"https://acct.r2.cloudflarestorage.com/bucket/key?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=[REDACTED]&X-Amz-Signature=[REDACTED]"},"status_code":302}`
	if got != want {
		t.Errorf("RedactAndTruncate =\n%s\nwant\n%s", got, want)
	}

	// A struct goes through RedactJSON and keeps its field order.
	type upload struct {
		URL  string `json:"url"`
		Note string `json:"note"`
	}
	if raw := Redact(upload{URL: "/v1/objects/upload?token=abc", Note: "see https://h.example/?token=abc"}).(json.RawMessage); string(raw) != `{"url":"/v1/objects/upload?token=[REDACTED]","note":"see https://h.example/?token=abc"}` {
		t.Errorf("struct: %s", raw)
	}

	for in, want := range map[string]string{
		"/v1/objects/download?token=abc":   "/v1/objects/download?token=[REDACTED]",
		"https://u:pw@h.example/":          "https://u:[REDACTED]@h.example/",
		"https://ghp_x@github.com/o/r.git": "https://[REDACTED]@github.com/o/r.git",
		"see https://h.example/?token=abc": "see https://h.example/?token=abc",
		"ana@example.com":                  "ana@example.com",
		"a?token=b":                        "a?token=b",
		"":                                 "",
	} {
		if got := Redact(in); got != want {
			t.Errorf("Redact(%q) = %q, want %q", in, got, want)
		}
	}

	out := redactedMap(t, map[string]any{"https://h.example/?token=abc": 1, "token": "https://h.example/?sig=a"})
	if out["https://h.example/?token=abc"] != float64(1) || out["token"] != RedactedPlaceholder {
		t.Errorf("keys are not URLs; a sensitive key replaces a URL whole: %v", out)
	}
}
