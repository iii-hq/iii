// Used only by the isolated opt-in full-engine regression.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
	"os"
	"strings"
	"time"
)

func main() {
	c := iii.New(os.Getenv("MOT4988_URL"), iii.WithName("mot4988-go"))
	if err := c.RegisterFunction("size::go", func(_ context.Context, data json.RawMessage) (any, error) {
		var req struct {
			Size int `json:"size"`
		}
		if err := json.Unmarshal(data, &req); err != nil {
			return nil, err
		}
		return strings.Repeat("x", req.Size), nil
	}); err != nil {
		panic(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()
	if err := c.Connect(ctx); err != nil {
		panic(err)
	}
	defer c.Close()
	call := func(target string, size int) (json.RawMessage, error) {
		return c.Trigger(ctx, iii.TriggerRequest{FunctionID: target, Data: json.RawMessage(fmt.Sprintf(`{"size":%d}`, size)), Timeout: 10 * time.Second})
	}
	for _, target := range []string{"size::rust", "size::node", "size::python"} {
		for attempt := 0; ; attempt++ {
			if _, err := call(target, 1); err == nil {
				break
			} else if attempt >= 50 {
				panic(err)
			}
			time.Sleep(100 * time.Millisecond)
		}
	}
	for _, target := range []string{"size::rust", "size::node", "size::python"} {
		_, err := call(target, 16*1024*1024)
		var ie *iii.InvocationError
		if !errors.As(err, &ie) || ie.Code != "payload_too_large" {
			panic(fmt.Sprintf("oversized %s: %v", target, err))
		}
		out, err := call(target, 2)
		if err != nil || string(out) != `"xx"` {
			panic(fmt.Sprintf("small %s: %s %v", target, out, err))
		}
	}
	// Near-limit engine-forwarded envelope; synthetic Rust socket test separately
	// proves >16 MiB receive capacity without assuming engine trace growth.
	near := 16*1024*1024 - 1024
	data, _ := json.Marshal(strings.Repeat("y", near))
	out, err := c.Trigger(ctx, iii.TriggerRequest{FunctionID: "size::rust-length", Data: data, Timeout: 10 * time.Second})
	if err != nil || string(out) != fmt.Sprint(near) {
		panic(fmt.Sprintf("near-limit: %s %v", out, err))
	}
	fmt.Println("MOT4988_GO_CROSS_SDK_OK")
	time.Sleep(8 * time.Second)
}
