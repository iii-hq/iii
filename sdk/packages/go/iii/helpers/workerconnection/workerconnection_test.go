package workerconnection

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
)

// jsonNames returns the JSON names of a struct's fields, in declaration order.
func jsonNames(v any) []string {
	t := reflect.TypeOf(v)
	names := make([]string, 0, t.NumField())
	for i := 0; i < t.NumField(); i++ {
		name, _, _ := strings.Cut(t.Field(i).Tag.Get("json"), ",")
		names = append(names, name)
	}
	return names
}

// TestJSONNamesMatchNode pins every field name against the Node types in
// sdk/packages/node/helpers/src/worker-connection-manager/index.ts (same order).
func TestJSONNamesMatchNode(t *testing.T) {
	cases := []struct {
		name string
		v    any
		want []string
	}{
		{"AuthInput", AuthInput{}, []string{"headers", "query_params", "ip_address"}},
		{"AuthResult", AuthResult{}, []string{
			"namespaces", "allowed_functions", "forbidden_functions", "allowed_trigger_types",
			"allow_trigger_type_registration", "allow_function_registration", "context",
			"function_registration_prefix",
		}},
		{"OnTriggerTypeRegistrationInput", OnTriggerTypeRegistrationInput{}, []string{"trigger_type_id", "description", "context"}},
		{"OnTriggerTypeRegistrationResult", OnTriggerTypeRegistrationResult{}, []string{"trigger_type_id", "description"}},
		{"OnTriggerRegistrationInput", OnTriggerRegistrationInput{}, []string{
			"trigger_id", "trigger_type", "function_id", "config", "metadata", "namespace", "context",
		}},
		{"OnTriggerRegistrationResult", OnTriggerRegistrationResult{}, []string{"trigger_id", "trigger_type", "function_id", "config"}},
		{"OnFunctionRegistrationInput", OnFunctionRegistrationInput{}, []string{
			"function_id", "description", "metadata", "namespace", "context",
		}},
		{"OnFunctionRegistrationResult", OnFunctionRegistrationResult{}, []string{"function_id", "description", "metadata"}},
	}
	for _, tc := range cases {
		if got := jsonNames(tc.v); !reflect.DeepEqual(got, tc.want) {
			t.Errorf("%s JSON names = %v, want %v", tc.name, got, tc.want)
		}
	}
}

// TestAuthInputDecodesEngineInput decodes the object the engine builds in
// Session::authenticate (engine/src/workers/worker/rbac_session.rs).
func TestAuthInputDecodesEngineInput(t *testing.T) {
	raw := `{"headers":{"authorization":"Bearer t"},"query_params":{"token":["a","b"]},"ip_address":"10.0.0.7"}`
	var in AuthInput
	if err := json.Unmarshal([]byte(raw), &in); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if in.Headers["authorization"] != "Bearer t" {
		t.Errorf("Headers = %v", in.Headers)
	}
	if got := in.QueryParams["token"]; !reflect.DeepEqual(got, []string{"a", "b"}) {
		t.Errorf("QueryParams[token] = %v", got)
	}
	if in.IPAddress != "10.0.0.7" {
		t.Errorf("IPAddress = %q", in.IPAddress)
	}
}

// TestAuthResultZeroValueKeepsEngineDefaults checks the zero value sends nothing the
// engine would read differently from an omitted field: Vec fields are omitted (the
// engine rejects null for them) and allowed_trigger_types is null (None: all allowed).
func TestAuthResultZeroValueKeepsEngineDefaults(t *testing.T) {
	out, err := json.Marshal(AuthResult{})
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"allowed_trigger_types":null}`; string(out) != want {
		t.Fatalf("zero AuthResult = %s, want %s", out, want)
	}
}

func TestAuthResultExplicitValues(t *testing.T) {
	out, err := json.Marshal(AuthResult{
		Namespaces:                   map[string][]string{"orders": {"svc::*"}},
		AllowedFunctions:             []string{"a::b"},
		ForbiddenFunctions:           []string{"a::secret"},
		AllowedTriggerTypes:          []string{},
		AllowTriggerTypeRegistration: true,
		AllowFunctionRegistration:    new(bool),
		Context:                      json.RawMessage(`{"user_id":"u1"}`),
		FunctionRegistrationPrefix:   "tenant1",
	})
	if err != nil {
		t.Fatal(err)
	}
	want := `{"namespaces":{"orders":["svc::*"]},"allowed_functions":["a::b"],` +
		`"forbidden_functions":["a::secret"],"allowed_trigger_types":[],` +
		`"allow_trigger_type_registration":true,"allow_function_registration":false,` +
		`"context":{"user_id":"u1"},"function_registration_prefix":"tenant1"}`
	if string(out) != want {
		t.Fatalf("AuthResult = %s\nwant         %s", out, want)
	}
}

// TestAuthResultDecodesDefaults mirrors the Rust auth_result_defaults_match_engine test:
// decoding {} leaves every field at the value that means "engine default".
func TestAuthResultDecodesDefaults(t *testing.T) {
	var r AuthResult
	if err := json.Unmarshal([]byte(`{}`), &r); err != nil {
		t.Fatal(err)
	}
	if r.Namespaces != nil || r.AllowedFunctions != nil || r.ForbiddenFunctions != nil ||
		r.AllowedTriggerTypes != nil || r.AllowTriggerTypeRegistration ||
		r.AllowFunctionRegistration != nil || r.Context != nil || r.FunctionRegistrationPrefix != "" {
		t.Fatalf("decoded {} = %+v, want the zero value", r)
	}
}

// TestHookInputsDecodeEngineInputs decodes the hook inputs exactly as the engine builds
// them (engine/src/engine/mod.rs), including null description and metadata.
func TestHookInputsDecodeEngineInputs(t *testing.T) {
	var tt OnTriggerTypeRegistrationInput
	mustUnmarshal(t, `{"trigger_type_id":"acme::webhook","description":"Acme hooks","context":{"k":1}}`, &tt)
	if tt.TriggerTypeID != "acme::webhook" || tt.Description != "Acme hooks" || string(tt.Context) != `{"k":1}` {
		t.Errorf("OnTriggerTypeRegistrationInput = %+v", tt)
	}

	var tr OnTriggerRegistrationInput
	mustUnmarshal(t, `{"trigger_id":"t1","trigger_type":"http","function_id":"api::get","config":{"api_path":"/x"},"metadata":null,"namespace":"default","context":{}}`, &tr)
	if tr.TriggerID != "t1" || tr.TriggerType != "http" || tr.FunctionID != "api::get" ||
		string(tr.Config) != `{"api_path":"/x"}` || tr.Namespace != "default" || string(tr.Context) != `{}` {
		t.Errorf("OnTriggerRegistrationInput = %+v", tr)
	}

	var fn OnFunctionRegistrationInput
	mustUnmarshal(t, `{"function_id":"svc::f","description":null,"metadata":{"team":"core"},"namespace":"orders","context":{"u":"x"}}`, &fn)
	if fn.FunctionID != "svc::f" || fn.Description != "" || string(fn.Metadata) != `{"team":"core"}` ||
		fn.Namespace != "orders" || string(fn.Context) != `{"u":"x"}` {
		t.Errorf("OnFunctionRegistrationInput = %+v", fn)
	}
}

// TestHookResultsOmitUnmappedFields checks an empty result keeps every original value
// (the engine only reads the keys that are present) and that a pointer to "" clears a
// description.
func TestHookResultsOmitUnmappedFields(t *testing.T) {
	for name, v := range map[string]any{
		"OnTriggerTypeRegistrationResult": OnTriggerTypeRegistrationResult{},
		"OnTriggerRegistrationResult":     OnTriggerRegistrationResult{},
		"OnFunctionRegistrationResult":    OnFunctionRegistrationResult{},
	} {
		out, err := json.Marshal(v)
		if err != nil {
			t.Fatal(err)
		}
		if string(out) != `{}` {
			t.Errorf("empty %s = %s, want {}", name, out)
		}
	}

	empty := ""
	out, err := json.Marshal(OnFunctionRegistrationResult{FunctionID: "svc::g", Description: &empty, Metadata: json.RawMessage(`{"a":1}`)})
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"function_id":"svc::g","description":"","metadata":{"a":1}}`; string(out) != want {
		t.Errorf("OnFunctionRegistrationResult = %s, want %s", out, want)
	}

	out, err = json.Marshal(OnTriggerRegistrationResult{TriggerID: "t2", TriggerType: "cron", FunctionID: "f", Config: json.RawMessage(`{"expression":"* * * * * *"}`)})
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"trigger_id":"t2","trigger_type":"cron","function_id":"f","config":{"expression":"* * * * * *"}}`; string(out) != want {
		t.Errorf("OnTriggerRegistrationResult = %s, want %s", out, want)
	}
}

func mustUnmarshal(t *testing.T, raw string, v any) {
	t.Helper()
	if err := json.Unmarshal([]byte(raw), v); err != nil {
		t.Fatalf("unmarshal %s: %v", raw, err)
	}
}
