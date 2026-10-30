package telemetry

import (
	"net/url"
	"testing"
)

// url.query: the values of the semconv sensitive keys become REDACTED, keys are matched
// exactly (after unescaping) and the rest of the query is kept byte for byte.
func TestRedactQuery(t *testing.T) {
	for in, want := range map[string]string{
		"":                  "",
		"a=1&b=two%20words": "a=1&b=two%20words",
		"a=1&AWSAccessKeyId=AKIA1&Signature=x%2By&sig=s&X-Goog-Signature=g&b=2": "a=1&AWSAccessKeyId=REDACTED&Signature=REDACTED&sig=REDACTED&X-Goog-Signature=REDACTED&b=2",
		"X%2DGoog%2DSignature=g":  "X%2DGoog%2DSignature=REDACTED",
		"sig=1&sig=2":             "sig=REDACTED&sig=REDACTED",
		"Signature=":              "Signature=REDACTED",
		"Signature":               "Signature",
		"signature=x&SIG=y&Sig=z": "signature=x&SIG=y&Sig=z",
		"sig=a=b":                 "sig=REDACTED",
	} {
		if got := redactQuery(in); got != want {
			t.Errorf("redactQuery(%q) = %q, want %q", in, got, want)
		}
	}
}

// url.full: userinfo becomes REDACTED:REDACTED (username alone too) and the query is
// redacted; the request's own URL is not modified.
func TestRedactURL(t *testing.T) {
	for in, want := range map[string]string{
		"https://user:pass@host.example/p?sig=1&q=2": "https://REDACTED:REDACTED@host.example/p?sig=REDACTED&q=2",
		"https://user@host.example/":                 "https://REDACTED:REDACTED@host.example/",
		"http://host.example:8080/a/b?x=1":           "http://host.example:8080/a/b?x=1",
	} {
		u, err := url.Parse(in)
		if err != nil {
			t.Fatal(err)
		}
		full, query := redactURL(u)
		if full != want {
			t.Errorf("redactURL(%q) = %q, want %q", in, full, want)
		}
		if wantQuery := mustParse(t, want).RawQuery; query != wantQuery {
			t.Errorf("redactURL(%q) query = %q, want %q", in, query, wantQuery)
		}
		if u.String() != in {
			t.Errorf("redactURL modified its input: %q", u.String())
		}
	}
}

func mustParse(t *testing.T, s string) *url.URL {
	t.Helper()
	u, err := url.Parse(s)
	if err != nil {
		t.Fatal(err)
	}
	return u
}
