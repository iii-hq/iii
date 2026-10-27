package iii

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

// A1: the default address is the IPv4 loopback the other SDKs use.
func TestDefaultEngineURL(t *testing.T) {
	if DefaultEngineURL != "ws://127.0.0.1:49134" {
		t.Errorf("DefaultEngineURL = %q, want ws://127.0.0.1:49134", DefaultEngineURL)
	}
}

// A1: III_URL, trimmed, else the default.
func TestEngineURLFromEnv(t *testing.T) {
	for _, tc := range []struct{ env, want string }{
		{"", DefaultEngineURL},
		{"   ", DefaultEngineURL},
		{"ws://engine:49134", "ws://engine:49134"},
		{"  ws://engine:1/x \n", "ws://engine:1/x"},
	} {
		t.Setenv("III_URL", tc.env)
		if got := EngineURLFromEnv(); got != tc.want {
			t.Errorf("III_URL=%q: EngineURLFromEnv() = %q, want %q", tc.env, got, tc.want)
		}
	}
}

// A1: an empty url resolves through III_URL then the default; an explicit one wins.
func TestNewResolvesEngineURL(t *testing.T) {
	t.Setenv("III_URL", "")
	if got := New("").Address(); got != DefaultEngineURL {
		t.Errorf("no III_URL: Address() = %q, want %q", got, DefaultEngineURL)
	}
	t.Setenv("III_URL", "ws://from-env:1")
	if got := New("ws://explicit:2").Address(); got != "ws://explicit:2" {
		t.Errorf("explicit url: Address() = %q, want ws://explicit:2", got)
	}

	// RegisterWorker("") dials the engine named by III_URL.
	m := newMockEngine(t)
	t.Setenv("III_URL", " "+m.url+" ")
	c := RegisterWorker("")
	t.Cleanup(func() { _ = c.Close() })
	if c.Address() != m.url {
		t.Errorf("Address() = %q, want %q", c.Address(), m.url)
	}
	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return indexOfFunctionID(msgs, FnRegisterWorker) >= 0
	}, 3*time.Second)
	if indexOfFunctionID(got, FnRegisterWorker) < 0 {
		t.Error("RegisterWorker(\"\") did not connect to the III_URL engine")
	}
}

// A7: Client.OpenWriter / OpenReader dial the client's address with its WithHeaders and
// the III_TOKEN bearer.
func TestClientOpenReaderWriterUseClientAddressAndHeaders(t *testing.T) {
	setToken(t, ptr("secret"))
	m := newMockEngine(t)
	c := New(m.url, WithHeaders(http.Header{"X-Other": {"kept"}}))
	ref := func(dir ChannelDirection) StreamChannelRef {
		return StreamChannelRef{ChannelID: "chan", AccessKey: "key", Direction: dir}
	}
	w := c.OpenWriter(ref(ChannelWrite))
	r := c.OpenReader(ref(ChannelRead))
	t.Cleanup(func() { _ = w.Close(); _ = r.Close() })

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := w.SendMessage(ctx, "hi"); err != nil {
		t.Fatalf("SendMessage: %v", err)
	}
	if _, err := r.ensureConnected(ctx); err != nil {
		t.Fatalf("reader connect: %v", err)
	}
	for i, dir := range []string{"write", "read"} {
		h := m.handshakeAt(i)
		if want := "/ws/channels/chan?key=key&dir=" + dir; h.uri != want {
			t.Errorf("%s uri = %q, want %q", dir, h.uri, want)
		}
		if got := h.header.Get("Authorization"); got != "Bearer secret" {
			t.Errorf("%s Authorization = %q, want Bearer secret", dir, got)
		}
		if got := h.header.Get("X-Other"); got != "kept" {
			t.Errorf("%s X-Other = %q, want kept", dir, got)
		}
	}
}

// workerRegisterFrame waits for the engine::workers::register frame.
func workerRegisterFrame(t *testing.T, m *mockEngine) map[string]json.RawMessage {
	t.Helper()
	got := m.waitFor(func(msgs []map[string]json.RawMessage) bool {
		return indexOfFunctionID(msgs, FnRegisterWorker) >= 0
	}, 3*time.Second)
	if i := indexOfFunctionID(got, FnRegisterWorker); i >= 0 {
		return got[i]
	}
	t.Fatal("no engine::workers::register frame")
	return nil
}

// A8: the whole engine::workers::register frame, with isolation (null when III_ISOLATION
// is unset) and telemetry built as the Node SDK's registerWorkerMetadata builds them
// (sdk/packages/node/iii/src/iii.ts). Framework is trimmed, as in Node.
func TestWorkerMetadataPayload(t *testing.T) {
	t.Setenv("III_ISOLATION", "")
	t.Setenv("III_NAMESPACE", "")
	t.Setenv("III_WORKER_NAME", "")
	m := newMockEngine(t)
	connectClient(t, m,
		WithName("meta-worker"),
		WithDescription("Orders worker"),
		WithTelemetry(TelemetryOptions{Language: "en-US", ProjectName: "orders", Framework: " custom "}))

	features, err := json.Marshal(sdkFeatures)
	if err != nil {
		t.Fatal(err)
	}
	want := fmt.Sprintf(`{"action":{"type":"void"},"data":{"description":"Orders worker","features":%s,`+
		`"isolation":null,"name":"meta-worker","os":%q,"pid":%d,"runtime":"go",`+
		`"telemetry":{"framework":"custom","language":"en-US","project_name":"orders"},"version":%q},`+
		`"function_id":"engine::workers::register","type":"invokefunction"}`,
		features, runtime.GOOS+" "+runtime.GOARCH, os.Getpid(), sdkVersion)
	if got := frameJSON(t, workerRegisterFrame(t, m)); got != sortedJSON(t, []byte(want)) {
		t.Errorf("engine::workers::register frame\n got %s\nwant %s", got, sortedJSON(t, []byte(want)))
	}
}

// A8: defaults: isolation from III_ISOLATION, language from LANG, project name from the
// nearest go.mod above the working directory, framework iii-go.
func TestWorkerMetadataTelemetryDefaults(t *testing.T) {
	t.Setenv("III_ISOLATION", "microvm")
	t.Setenv("LANG", "pt_BR.UTF-8")
	t.Setenv("LC_ALL", "")
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "go.mod"), []byte("module example.com/acme/orders/v2\n\ngo 1.24\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	wd := filepath.Join(root, "cmd", "worker")
	if err := os.MkdirAll(wd, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Chdir(wd)

	m := newMockEngine(t)
	connectClient(t, m)
	var data map[string]json.RawMessage
	if err := json.Unmarshal(workerRegisterFrame(t, m)["data"], &data); err != nil {
		t.Fatal(err)
	}
	if got := string(data["isolation"]); got != `"microvm"` {
		t.Errorf("isolation = %s, want \"microvm\"", got)
	}
	const want = `{"framework":"iii-go","language":"pt_BR","project_name":"orders"}`
	if got := sortedJSON(t, data["telemetry"]); got != want {
		t.Errorf("telemetry = %s, want %s", got, want)
	}
}

// A8: language falls back to LC_ALL and is omitted when neither variable is set.
func TestDefaultTelemetryLanguage(t *testing.T) {
	for _, tc := range []struct{ lang, lcAll, want string }{
		{"en_US.UTF-8", "", "en_US"},
		{"de_DE", "fr_FR.UTF-8", "de_DE"},
		{"", "C.UTF-8", "C"},
		{"", "", ""},
	} {
		t.Setenv("LANG", tc.lang)
		t.Setenv("LC_ALL", tc.lcAll)
		if got := defaultTelemetryLanguage(); got != tc.want {
			t.Errorf("LANG=%q LC_ALL=%q: language = %q, want %q", tc.lang, tc.lcAll, got, tc.want)
		}
	}
	t.Setenv("LANG", "")
	t.Setenv("LC_ALL", "")
	c := New("ws://127.0.0.1:0")
	raw, err := json.Marshal(c.resolveTelemetry())
	if err != nil {
		t.Fatal(err)
	}
	var telemetry map[string]json.RawMessage
	_ = json.Unmarshal(raw, &telemetry)
	if _, ok := telemetry["language"]; ok {
		t.Errorf("telemetry %s carries a language with LANG and LC_ALL unset", raw)
	}
}

// A8: the project name is the last module path element of the nearest go.mod.
func TestDetectProjectName(t *testing.T) {
	write := func(dir, gomod string) string {
		if err := os.WriteFile(filepath.Join(dir, "go.mod"), []byte(gomod), 0o644); err != nil {
			t.Fatal(err)
		}
		return dir
	}
	for _, tc := range []struct{ gomod, want string }{
		{"module github.com/iii-hq/iii/sdk/packages/go/iii\n", "iii"},
		{"// header\nmodule \"example.com/quoted\" // trailing\n", "quoted"},
		{"module (\n\texample.com/blocky/v3\n)\n", "blocky"},
		{"module orders\n", "orders"},
		{"go 1.24\n", ""},
	} {
		if got := detectProjectName(write(t.TempDir(), tc.gomod)); got != tc.want {
			t.Errorf("go.mod %q: project = %q, want %q", tc.gomod, got, tc.want)
		}
	}

	// Walks up to the nearest go.mod, and stops there.
	root := write(t.TempDir(), "module example.com/outer\n")
	inner := write(mkdirAll(t, filepath.Join(root, "inner")), "module example.com/inner\n")
	if got := detectProjectName(mkdirAll(t, filepath.Join(inner, "a", "b"))); got != "inner" {
		t.Errorf("nested: project = %q, want inner", got)
	}

	// No go.mod anywhere above: empty (only checkable when the temp dir has none).
	bare := t.TempDir()
	for dir := bare; ; dir = filepath.Dir(dir) {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			t.Logf("skipping the no-go.mod case: %s has a go.mod", dir)
			return
		}
		if filepath.Dir(dir) == dir {
			break
		}
	}
	if got := detectProjectName(bare); got != "" {
		t.Errorf("no go.mod: project = %q, want empty", got)
	}
}

func mkdirAll(t *testing.T, dir string) string {
	t.Helper()
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	return dir
}

func TestProjectNameFromModulePath(t *testing.T) {
	for path, want := range map[string]string{
		"github.com/iii-hq/iii/sdk/packages/go/iii": "iii",
		"example.com/shop/v2":                       "shop",
		"example.com/shop/v10":                      "shop",
		"example.com/api/v1":                        "v1",
		"example.com/api/v02":                       "v02",
		"gopkg.in/yaml.v3":                          "yaml.v3",
		"orders":                                    "orders",
		"":                                          "",
	} {
		if got := projectNameFromModulePath(path); got != want {
			t.Errorf("projectNameFromModulePath(%q) = %q, want %q", path, got, want)
		}
	}
}
