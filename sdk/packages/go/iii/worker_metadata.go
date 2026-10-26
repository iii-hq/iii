package iii

import (
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

// This file builds the parts of the engine::workers::register payload that describe
// where the worker runs: isolation and telemetry, as the Node SDK's
// registerWorkerMetadata does (sdk/packages/node/iii/src/iii.ts).

// TelemetryOptions is what the worker reports about its language, project and framework
// in the telemetry object of engine::workers::register, shown by engine::workers::list
// and engine::workers::info. A blank field takes its default. Mirrors TelemetryOptions
// in the Node and Rust SDKs.
type TelemetryOptions struct {
	// Language reported for the worker. Default: the locale of LANG (else LC_ALL)
	// without its encoding, for example "en_US" for LANG=en_US.UTF-8, which is the Node
	// SDK's fallback; omitted when neither variable is set.
	Language string
	// ProjectName is the project the worker belongs to. Default: the last element of
	// the module path in the nearest go.mod, searched from the working directory
	// upwards, skipping a major-version suffix such as /v2; omitted when there is no
	// go.mod.
	ProjectName string
	// Framework the worker is built with. Default: "iii-go". Surrounding spaces are
	// trimmed.
	Framework string
}

// WithTelemetry sets what the worker reports about its language, project and framework
// on every connection. Blank fields keep their defaults (see [TelemetryOptions]).
func WithTelemetry(t TelemetryOptions) Option {
	return func(c *Client) { c.telemetry = t }
}

// defaultTelemetryFramework is the telemetry framework of a Go worker (Node: "iii-node",
// Python: "iii-py", Rust: "iii-rust").
const defaultTelemetryFramework = "iii-go"

// workerTelemetry is the telemetry object of engine::workers::register. As in the Node
// SDK, language and project_name are omitted when unknown and framework is always sent.
type workerTelemetry struct {
	Language    string `json:"language,omitempty"`
	ProjectName string `json:"project_name,omitempty"`
	Framework   string `json:"framework"`
}

// resolveTelemetry is WithTelemetry with the defaults filled in, resolved on each
// connection as the Node SDK does.
func (c *Client) resolveTelemetry() workerTelemetry {
	t := workerTelemetry{
		Language:    c.telemetry.Language,
		ProjectName: c.telemetry.ProjectName,
		Framework:   strings.TrimSpace(c.telemetry.Framework),
	}
	if t.Language == "" {
		t.Language = defaultTelemetryLanguage()
	}
	if t.ProjectName == "" {
		if wd, err := os.Getwd(); err == nil {
			t.ProjectName = detectProjectName(wd)
		}
	}
	if t.Framework == "" {
		t.Framework = defaultTelemetryFramework
	}
	return t
}

// defaultTelemetryLanguage is the locale of LANG, else LC_ALL, without its encoding
// ("en_US.UTF-8" -> "en_US"), or "" when neither is set. Go has no Intl locale, so this
// is the Node SDK's LANG fallback, with the Rust SDK's LC_ALL after it.
func defaultTelemetryLanguage() string {
	for _, name := range []string{"LANG", "LC_ALL"} {
		if v := os.Getenv(name); v != "" {
			lang, _, _ := strings.Cut(v, ".")
			return lang
		}
	}
	return ""
}

// isolationFromEnv is the isolation the supervisor runs this worker under
// (III_ISOLATION), or nil, sent as null, when it is unset or empty, like the Node SDK's
// `process.env.III_ISOLATION || null`.
func isolationFromEnv() *string {
	if v := os.Getenv("III_ISOLATION"); v != "" {
		return &v
	}
	return nil
}

// detectProjectName returns the project name of the nearest go.mod at or above dir: the
// last element of its module path, skipping a major-version suffix ("example.com/shop/v2"
// -> "shop"). It returns "" when there is no go.mod or it declares no module.
func detectProjectName(dir string) string {
	for {
		if data, err := os.ReadFile(filepath.Join(dir, "go.mod")); err == nil {
			return projectNameFromModulePath(goModModulePath(string(data)))
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return ""
		}
		dir = parent
	}
}

// goModModulePath returns the path of the module directive of a go.mod file, in either
// the line or the block form, quoted or not, or "" when there is none.
func goModModulePath(gomod string) string {
	inBlock := false
	for _, line := range strings.Split(gomod, "\n") {
		line, _, _ = strings.Cut(line, "//")
		fields := strings.Fields(line)
		switch {
		case len(fields) == 0:
			continue
		case inBlock:
			if fields[0] == ")" {
				return ""
			}
			return unquoteModulePath(fields[0])
		case fields[0] != "module":
			continue
		case len(fields) == 1:
			return ""
		case fields[1] == "(":
			inBlock = true
		default:
			return unquoteModulePath(fields[1])
		}
	}
	return ""
}

func unquoteModulePath(s string) string {
	if unquoted, err := strconv.Unquote(s); err == nil {
		return unquoted
	}
	return s
}

// projectNameFromModulePath is the last element of a module path, or the one before it
// when the last is a major-version suffix (v2, v3, ...).
func projectNameFromModulePath(modulePath string) string {
	elems := strings.Split(strings.Trim(modulePath, "/"), "/")
	name := elems[len(elems)-1]
	if len(elems) > 1 && isMajorVersionSuffix(name) {
		name = elems[len(elems)-2]
	}
	return name
}

// isMajorVersionSuffix reports whether elem is "vN" with N >= 2, the form Go module
// paths use for major versions.
func isMajorVersionSuffix(elem string) bool {
	n, err := strconv.Atoi(strings.TrimPrefix(elem, "v"))
	return strings.HasPrefix(elem, "v") && err == nil && n >= 2 && !strings.HasPrefix(elem, "v0")
}
