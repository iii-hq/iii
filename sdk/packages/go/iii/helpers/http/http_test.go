package http

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

// jsonNames returns the JSON names of a struct's exported fields, in declaration order.
func jsonNames(v any) []string {
	t := reflect.TypeOf(v)
	var names []string
	for i := 0; i < t.NumField(); i++ {
		if !t.Field(i).IsExported() {
			continue
		}
		name, _, _ := strings.Cut(t.Field(i).Tag.Get("json"), ",")
		names = append(names, name)
	}
	return names
}

// TestJSONNamesMatchNode pins the field names against Node HttpRequest / HttpResponse
// (helpers/src/http/index.ts) and InternalHttpRequest / StreamRequest (iii/src/types.ts).
// path is the one addition, from Rust HttpRequest and the http worker's payload.
func TestJSONNamesMatchNode(t *testing.T) {
	cases := []struct {
		name string
		v    any
		want []string
	}{
		{"Request", Request[json.RawMessage]{}, []string{"path_params", "query_params", "body", "headers", "path", "method", "request_body"}},
		{"Response", Response[any]{}, []string{"status_code", "headers", "body"}},
		{"InternalRequest", InternalRequest{}, []string{"path_params", "query_params", "body", "headers", "path", "method", "response", "request_body"}},
		{"StreamRequest", StreamRequest{}, []string{"path_params", "query_params", "body", "headers", "path", "method", "-"}},
		{"setStatusMessage", setStatusMessage{}, []string{"type", "status_code"}},
		{"setHeadersMessage", setHeadersMessage{}, []string{"type", "headers"}},
	}
	for _, tc := range cases {
		if got := jsonNames(tc.v); !reflect.DeepEqual(got, tc.want) {
			t.Errorf("%s JSON names = %v, want %v", tc.name, got, tc.want)
		}
	}
}

func TestMethodConstants(t *testing.T) {
	got := []Method{MethodGet, MethodPost, MethodPut, MethodPatch, MethodDelete}
	want := []string{"GET", "POST", "PUT", "PATCH", "DELETE"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("methods = %v, want %v", got, want)
	}
	// Method is an alias of string: it assigns to the core ref without a conversion.
	cfg := InvocationConfig{URL: "https://example.com/fn", Method: MethodPut}
	var ref iii.HTTPInvocationRef = cfg
	if ref.Method != "PUT" {
		t.Fatalf("Method = %q", ref.Method)
	}
}

// TestInvocationConfigJSON pins Node HttpInvocationConfig names: url, method,
// timeout_ms, headers, auth.
func TestInvocationConfigJSON(t *testing.T) {
	timeout := uint64(5000)
	out, err := json.Marshal(InvocationConfig{
		URL:       "https://example.com/fn",
		Method:    MethodPost,
		TimeoutMs: &timeout,
		Headers:   map[string]string{"x-env": "prod"},
		Auth:      json.RawMessage(`{"type":"bearer","token_key":"FN_TOKEN"}`),
	})
	if err != nil {
		t.Fatal(err)
	}
	want := `{"url":"https://example.com/fn","method":"POST","timeout_ms":5000,"headers":{"x-env":"prod"},"auth":{"type":"bearer","token_key":"FN_TOKEN"}}`
	if string(out) != want {
		t.Fatalf("InvocationConfig = %s\nwant              %s", out, want)
	}
}

func TestMultiValueJSON(t *testing.T) {
	decode := []struct {
		in   string
		want MultiValue
	}{
		{`"a"`, MultiValue{"a"}},
		{`["a","b"]`, MultiValue{"a", "b"}},
		{`[]`, MultiValue{}},
		{`null`, nil},
	}
	for _, tc := range decode {
		var got MultiValue
		if err := json.Unmarshal([]byte(tc.in), &got); err != nil {
			t.Fatalf("unmarshal %s: %v", tc.in, err)
		}
		if !reflect.DeepEqual(got, tc.want) {
			t.Errorf("unmarshal %s = %#v, want %#v", tc.in, got, tc.want)
		}
	}
	for _, bad := range []string{`1`, `{"a":1}`, `[1]`} {
		var got MultiValue
		if err := json.Unmarshal([]byte(bad), &got); err == nil {
			t.Errorf("unmarshal %s: want an error, got %#v", bad, got)
		}
	}

	encode := []struct {
		in   MultiValue
		want string
	}{
		{MultiValue{"a"}, `"a"`},
		{MultiValue{"a", "b"}, `["a","b"]`},
		{MultiValue{}, `[]`},
		{nil, `null`},
	}
	for _, tc := range encode {
		out, err := json.Marshal(tc.in)
		if err != nil {
			t.Fatal(err)
		}
		if string(out) != tc.want {
			t.Errorf("marshal %#v = %s, want %s", tc.in, out, tc.want)
		}
	}
	if (MultiValue{}).First() != "" || (MultiValue{"x", "y"}).First() != "x" {
		t.Error("First")
	}
}

// TestRequestDecodesWorkerPayload decodes a buffered Request from the http worker's
// payload, and from the Node shape where a header or query value is an array.
func TestRequestDecodesWorkerPayload(t *testing.T) {
	raw := `{"query_params":{"tag":["a","b"],"q":"go"},"path_params":{"id":"7"},` +
		`"headers":{"accept":"*/*","x-multi":["1","2"]},"path":"/items/:id","method":"PUT",` +
		`"body":{"name":"widget"},` +
		`"request_body":{"channel_id":"c1","access_key":"k1","direction":"read"},` +
		`"response":{"channel_id":"c2","access_key":"k2","direction":"write"}}`
	var req Request[struct {
		Name string `json:"name"`
	}]
	if err := json.Unmarshal([]byte(raw), &req); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if req.Body.Name != "widget" || req.Method != "PUT" || req.Path != "/items/:id" || req.PathParams["id"] != "7" {
		t.Errorf("request = %+v", req)
	}
	if !reflect.DeepEqual(req.QueryParams["tag"], MultiValue{"a", "b"}) || req.QueryParams["q"].First() != "go" {
		t.Errorf("query = %v", req.QueryParams)
	}
	if !reflect.DeepEqual(req.Headers["x-multi"], MultiValue{"1", "2"}) || req.Headers["accept"].First() != "*/*" {
		t.Errorf("headers = %v", req.Headers)
	}
	if req.RequestBody == nil || *req.RequestBody != (iii.StreamChannelRef{ChannelID: "c1", AccessKey: "k1", Direction: iii.ChannelRead}) {
		t.Errorf("request_body = %+v", req.RequestBody)
	}

	var in InternalRequest
	if err := json.Unmarshal([]byte(raw), &in); err != nil {
		t.Fatalf("unmarshal InternalRequest: %v", err)
	}
	if in.Response.Direction != iii.ChannelWrite || in.Response.ChannelID != "c2" || in.RequestBody.ChannelID != "c1" ||
		string(in.Body) != `{"name":"widget"}` {
		t.Errorf("InternalRequest = %+v", in)
	}
}

// TestResponseJSON checks the buffered response the http worker reads
// (HttpResponse::from_function_return): optional headers and body are omitted.
func TestResponseJSON(t *testing.T) {
	out, err := json.Marshal(Response[any]{StatusCode: 204})
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"status_code":204}`; string(out) != want {
		t.Errorf("minimal = %s, want %s", out, want)
	}
	out, err = json.Marshal(Response[string]{StatusCode: 200, Headers: map[string]string{"content-type": "text/plain"}, Body: "hi"})
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"status_code":200,"headers":{"content-type":"text/plain"},"body":"hi"}`; string(out) != want {
		t.Errorf("full = %s, want %s", out, want)
	}
}
