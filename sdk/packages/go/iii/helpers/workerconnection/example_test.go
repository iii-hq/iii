package workerconnection_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	iii "github.com/iii-hq/iii/sdk/packages/go/iii"
	"github.com/iii-hq/iii/sdk/packages/go/iii/helpers/workerconnection"
)

// authorize is an RBAC auth function: it admits connections that carry the expected
// token and scopes them to the orders namespace.
func authorize(_ context.Context, in workerconnection.AuthInput) (workerconnection.AuthResult, error) {
	tokens := in.QueryParams["token"]
	if len(tokens) == 0 || tokens[0] != "dev-token" {
		return workerconnection.AuthResult{}, errors.New("unauthorized")
	}
	return workerconnection.AuthResult{
		Namespaces:                map[string][]string{"orders": {"orders::*"}},
		ForbiddenFunctions:        []string{"orders::admin"},
		AllowFunctionRegistration: new(bool), // deny registering new functions
		Context:                   json.RawMessage(`{"source":"browser"}`),
	}, nil
}

// ExampleAuthResult runs an auth function against an upgrade request and prints the
// AuthResult the engine receives.
func ExampleAuthResult() {
	res, err := authorize(context.Background(), workerconnection.AuthInput{
		Headers:     map[string]string{"user-agent": "browser"},
		QueryParams: map[string][]string{"token": {"dev-token"}},
		IPAddress:   "127.0.0.1",
	})
	if err != nil {
		panic(err)
	}
	out, _ := json.Marshal(res)
	fmt.Println(string(out))
	// Output:
	// {"namespaces":{"orders":["orders::*"]},"forbidden_functions":["orders::admin"],"allowed_trigger_types":null,"allow_function_registration":false,"context":{"source":"browser"}}
}

// ExampleOnFunctionRegistrationResult registers the auth function and an
// on_function_registration hook that prefixes every function a browser registers.
func ExampleOnFunctionRegistrationResult() {
	client := iii.RegisterWorker(iii.DefaultEngineURL)
	defer client.Close()

	_ = iii.RegisterFunctionTyped(client, "auth::browser", authorize)
	_ = iii.RegisterFunctionTyped(client, "auth::on-function-registration",
		func(_ context.Context, in workerconnection.OnFunctionRegistrationInput) (workerconnection.OnFunctionRegistrationResult, error) {
			if strings.HasPrefix(in.FunctionID, "engine::") {
				return workerconnection.OnFunctionRegistrationResult{}, errors.New("reserved prefix")
			}
			return workerconnection.OnFunctionRegistrationResult{FunctionID: "browser::" + in.FunctionID}, nil
		})
}
