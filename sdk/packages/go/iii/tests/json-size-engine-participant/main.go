// Used only by the isolated opt-in full-engine fragmentation regression.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"regexp"
	"strconv"
	"strings"
	"time"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
)

const (
	messageLimitBytes = 64 * 1024 * 1024
	payloadBytes      = 40 * 1024 * 1024
	patternPeriod     = 97
	patternAlphabet   = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
)

var sizeMessage = regexp.MustCompile(`^Serialized JSON envelope is (\d+) bytes; limit is 67108864 bytes\. Use channels for larger payloads: https://iii\.dev/docs/creating-workers/channels#payload-size$`)

func expectedByte(index int, marker byte) byte {
	return patternAlphabet[(index%patternPeriod+int(marker))%len(patternAlphabet)]
}
func pattern(size int, marker byte) string {
	var output strings.Builder
	output.Grow(size)
	for index := 0; index < size; index++ {
		output.WriteByte(expectedByte(index, marker))
	}
	return output.String()
}
func verifyPayload(encoded json.RawMessage, size int, marker byte) error {
	var value string
	if err := json.Unmarshal(encoded, &value); err != nil {
		return err
	}
	if len(value) != size {
		return fmt.Errorf("payload length %d, want %d", len(value), size)
	}
	for index := range value {
		if value[index] != expectedByte(index, marker) {
			return fmt.Errorf("payload content/order mismatch at byte %d", index)
		}
	}
	return nil
}

func main() {
	client := iii.New(os.Getenv("JSON_SIZE_TEST_URL"), iii.WithName("json-size-go"))
	register := func(id string, handler iii.Handler) {
		if _, err := client.RegisterFunction(id, handler); err != nil {
			panic(err)
		}
	}
	register("size::go-ready", func(context.Context, json.RawMessage) (any, error) { return true, nil })
	register("size::go", func(_ context.Context, data json.RawMessage) (any, error) {
		var value string
		if err := json.Unmarshal(data, &value); err != nil {
			return nil, err
		}
		return value, nil
	})
	register("size::go-roundtrip", func(ctx context.Context, data json.RawMessage) (any, error) {
		var request struct {
			Target string `json:"target"`
			Size   int    `json:"size"`
			Marker string `json:"marker"`
		}
		if err := json.Unmarshal(data, &request); err != nil {
			return nil, err
		}
		if request.Size != payloadBytes || len(request.Marker) != 1 {
			return nil, fmt.Errorf("invalid Go caller request")
		}
		encoded, err := json.Marshal(pattern(request.Size, request.Marker[0]))
		if err != nil {
			return nil, err
		}
		result, err := client.Trigger(ctx, iii.TriggerRequest{FunctionID: request.Target, Payload: encoded, Timeout: 120 * time.Second})
		if err != nil {
			return nil, err
		}
		if err := verifyPayload(result, request.Size, request.Marker[0]); err != nil {
			return nil, err
		}
		fmt.Println("JSON_SIZE_GO_CROSS_SDK_OK")
		return true, nil
	})
	register("size::go-local-limit", func(ctx context.Context, _ json.RawMessage) (any, error) {
		encoded, err := json.Marshal(pattern(messageLimitBytes, 'Z'))
		if err != nil {
			return nil, err
		}
		_, err = client.Trigger(ctx, iii.TriggerRequest{FunctionID: "size::rust", Payload: encoded})
		var inv *iii.InvocationError
		if !errors.As(err, &inv) || inv.Code != "payload_too_large" {
			return nil, fmt.Errorf("expected payload_too_large, got %v", err)
		}
		match := sizeMessage.FindStringSubmatch(inv.Message)
		if match == nil {
			return nil, fmt.Errorf("unexpected size error: %s", inv.Message)
		}
		reported, parseErr := strconv.Atoi(match[1])
		if parseErr != nil || reported <= messageLimitBytes {
			return nil, fmt.Errorf("reported size %q not oversized", match[1])
		}
		small, err := client.Trigger(ctx, iii.TriggerRequest{FunctionID: "size::rust", Payload: json.RawMessage(`"after-go-limit"`)})
		if err != nil || string(small) != `"after-go-limit"` {
			return nil, fmt.Errorf("call after local reject: %s %v", small, err)
		}
		return true, nil
	})
	ctx, cancel := context.WithTimeout(context.Background(), 180*time.Second)
	defer cancel()
	if err := client.Connect(ctx); err != nil {
		panic(err)
	}
	defer client.Close()
	fmt.Println("JSON_SIZE_GO_PARTICIPANT_READY")
	select {}
}
