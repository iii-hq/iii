package http_test

import (
	"context"
	"encoding/json"
	"fmt"
	"io"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
	iiihttp "github.com/iii-hq/iii/sdk/packages/go/iii/helpers/http"
)

// ExampleWrap registers a streaming HTTP function: it reads the raw request body from
// req, then sets the status and headers and streams the answer to res.
func ExampleWrap() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	_, _ = client.RegisterFunction("api::echo", iiihttp.Wrap(client,
		func(ctx context.Context, req *iiihttp.StreamRequest, res *iiihttp.StreamResponse) (any, error) {
			body, err := io.ReadAll(req)
			if err != nil {
				return nil, err
			}
			if err := res.Status(200); err != nil {
				return nil, err
			}
			if err := res.Headers(map[string]string{"content-type": "text/plain"}); err != nil {
				return nil, err
			}
			if _, err := fmt.Fprintf(res, "%s %s: %d bytes\n", req.Method, req.Path, len(body)); err != nil {
				return nil, err
			}
			return nil, nil // the stream is closed when the handler returns
		}))

	// Route POST /echo to the function through the http worker's request source.
	_, _ = client.Observe(iii.Source{
		FunctionID: "http::on-request",
		Args:       iii.Args{"method": "POST", "path": "/echo"},
	}, "api::echo")
}

// ExampleWrap_buffered answers without streaming by returning a Response. The handler is
// called here with a plain payload, as a direct call would send it.
func ExampleWrap_buffered() {
	client := iii.New(iii.DefaultEngineURL)
	defer client.Close()

	handler := iiihttp.Wrap(client,
		func(ctx context.Context, req *iiihttp.StreamRequest, res *iiihttp.StreamResponse) (any, error) {
			return iiihttp.Response[map[string]string]{
				StatusCode: 200,
				Body:       map[string]string{"hello": req.QueryParams["name"].First()},
			}, nil
		})

	result, err := handler(context.Background(), json.RawMessage(`{"method":"GET","query_params":{"name":"ada"}}`))
	if err != nil {
		panic(err)
	}
	out, _ := json.Marshal(result)
	fmt.Println(string(out))
	// Output: {"status_code":200,"body":{"hello":"ada"}}
}

// ExampleRequest decodes a buffered request with a typed body.
func ExampleRequest() {
	type order struct {
		Item string `json:"item"`
	}
	payload := json.RawMessage(`{"method":"POST","path":"/orders/:id","path_params":{"id":"9"},` +
		`"query_params":{"tag":["a","b"]},"headers":{"accept":"application/json"},"body":{"item":"book"}}`)

	var req iiihttp.Request[order]
	if err := json.Unmarshal(payload, &req); err != nil {
		panic(err)
	}
	fmt.Println(req.Method, req.Path, req.PathParams["id"], req.Body.Item)
	fmt.Println(req.QueryParams["tag"], req.Headers["accept"].First())
	// Output:
	// POST /orders/:id 9 book
	// [a b] application/json
}

// ExampleInvocationConfig describes a function the engine invokes over HTTP.
func ExampleInvocationConfig() {
	timeout := uint64(10_000)
	cfg := iiihttp.InvocationConfig{
		URL:       "https://example.com/lambda/report",
		Method:    iiihttp.MethodPost,
		TimeoutMs: &timeout,
		Auth:      json.RawMessage(`{"type":"hmac","secret_key":"REPORT_SECRET"}`),
	}
	out, _ := json.Marshal(cfg)
	fmt.Println(string(out))
	// Output: {"url":"https://example.com/lambda/report","method":"POST","timeout_ms":10000,"auth":{"type":"hmac","secret_key":"REPORT_SECRET"}}
}
