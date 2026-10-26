package main

import (
	"bytes"
	"testing"
)

func extractSample(t *testing.T) *Module {
	t.Helper()
	m, err := Extract("testdata/sample")
	if err != nil {
		t.Fatal(err)
	}
	return m
}

func findPackage(t *testing.T, m *Module, rel string) Package {
	t.Helper()
	for _, p := range m.Packages {
		if p.RelPath == rel {
			return p
		}
	}
	t.Fatalf("package %q not found", rel)
	return Package{}
}

func findType(t *testing.T, p Package, name string) Type {
	t.Helper()
	for _, ty := range p.Types {
		if ty.Name == name {
			return ty
		}
	}
	t.Fatalf("type %q not found", name)
	return Type{}
}

func findFunc(t *testing.T, fs []Func, name string) Func {
	t.Helper()
	for _, f := range fs {
		if f.Name == name {
			return f
		}
	}
	t.Fatalf("func %q not found", name)
	return Func{}
}

func TestPackagesSkipInternalMainAndNestedModules(t *testing.T) {
	m := extractSample(t)
	if m.Module != "example.com/sample" || m.GoVersion != "1.24" {
		t.Fatalf("module = %q %q", m.Module, m.GoVersion)
	}
	var got []string
	for _, p := range m.Packages {
		got = append(got, p.RelPath)
	}
	if len(got) != 2 || got[0] != "" || got[1] != "helpers/http" {
		t.Fatalf("packages = %q, want [\"\" \"helpers/http\"]", got)
	}
	root := findPackage(t, m, "")
	if root.ImportPath != "example.com/sample" || root.Synopsis != "Package sample is a fixture for the godoc-json tests." {
		t.Fatalf("root = %q %q", root.ImportPath, root.Synopsis)
	}
	for _, f := range root.Funcs {
		if f.Name == "WindowsOnly" {
			t.Fatal("a windows-only file was documented")
		}
	}
}

func TestFuncsMethodsAndDeprecation(t *testing.T) {
	root := findPackage(t, extractSample(t), "")
	client := findType(t, root, "Client")
	if len(client.Fields) != 0 || !client.Incomplete {
		t.Fatalf("Client fields = %+v incomplete = %v", client.Fields, client.Incomplete)
	}
	newFn := findFunc(t, client.Funcs, "New")
	if newFn.Signature != "func New(url string, opts ...Option) *Client" {
		t.Fatalf("New signature = %q", newFn.Signature)
	}
	if len(newFn.Params) != 2 || !newFn.Params[1].Variadic || newFn.Params[1].Type != "...Option" {
		t.Fatalf("New params = %+v", newFn.Params)
	}
	legacy := findFunc(t, client.Methods, "Legacy")
	if legacy.Signature != "func (c *Client) Legacy(id, name string) error" {
		t.Fatalf("multi-line signature not joined: %q", legacy.Signature)
	}
	if legacy.Recv != "*Client" || legacy.FuncType != "func(id, name string) error" {
		t.Fatalf("Legacy recv/type = %q %q", legacy.Recv, legacy.FuncType)
	}
	dep := legacy.Doc.Deprecated
	if dep == nil || len(dep.Spans) != 3 || dep.Spans[0].Text != "use " || dep.Spans[1].Kind != "doclink" || dep.Spans[1].Text != "Client.Send" || dep.Spans[2].Text != "." {
		t.Fatalf("Legacy deprecation = %+v", dep)
	}
	if len(legacy.Doc.Blocks) != 1 {
		t.Fatalf("deprecation paragraph left in blocks: %+v", legacy.Doc.Blocks)
	}
	send := findFunc(t, client.Methods, "Send")
	if len(send.Doc.Blocks) != 2 || send.Doc.Blocks[1].Kind != "code" || send.Doc.Blocks[1].Code != "c.Send(nil)" {
		t.Fatalf("Send blocks = %+v", send.Doc.Blocks)
	}
	if len(send.Examples) != 1 || send.Examples[0].Code != "c := sample.New(\"\")\n// send nothing\n_ = c.Send(nil)" {
		t.Fatalf("Send examples = %+v", send.Examples)
	}
	if len(root.Examples) != 1 || root.Examples[0].Code != "c := sample.New(sample.DefaultURL)\n_ = c.Send(nil)" {
		t.Fatalf("package examples = %+v", root.Examples)
	}
}

func TestTypesFieldsAndValues(t *testing.T) {
	root := findPackage(t, extractSample(t), "")

	msg := findType(t, root, "Message")
	if msg.Kind != "struct" || msg.Decl != "" || !msg.Incomplete {
		t.Fatalf("Message kind/decl/incomplete = %q %q %v", msg.Kind, msg.Decl, msg.Incomplete)
	}
	var names []string
	for _, f := range msg.Fields {
		names = append(names, f.Name)
	}
	if len(names) != 5 || names[0] != "ID" || names[3] != "A" || names[4] != "B" {
		t.Fatalf("Message fields = %q", names)
	}
	if tag := msg.Fields[1].JSONTag; tag == nil || *tag != "note,omitempty" || msg.Fields[1].Type != "*string" {
		t.Fatalf("Note field = %+v", msg.Fields[1])
	}
	if msg.Fields[2].Doc.Raw != "trailing comment doc\n" {
		t.Fatalf("trailing field comment = %q", msg.Fields[2].Doc.Raw)
	}
	if msg.Fields[3].JSONTag != nil {
		t.Fatalf("untagged field has a json tag: %+v", msg.Fields[3])
	}

	if alias := findType(t, root, "Alias"); alias.Kind != "alias" || alias.Decl != "type Alias = Message" {
		t.Fatalf("Alias = %q %q", alias.Kind, alias.Decl)
	}
	opt := findType(t, root, "Option")
	if opt.Kind != "func" || opt.Decl != "type Option func(*Client)" || len(opt.Funcs) != 1 || opt.Funcs[0].Name != "WithName" {
		t.Fatalf("Option = %+v", opt)
	}
	doer := findType(t, root, "Doer")
	if len(doer.InterfaceMethods) != 1 || doer.InterfaceMethods[0].Type != "func(n int) (ok bool, err error)" {
		t.Fatalf("Doer methods = %+v", doer.InterfaceMethods)
	}

	level := findType(t, root, "Level")
	if len(level.Consts) != 1 || len(level.Consts[0].Specs) != 2 {
		t.Fatalf("Level consts = %+v", level.Consts)
	}
	if low, high := level.Consts[0].Specs[0], level.Consts[0].Specs[1]; low.Value != "iota" || low.Type != "Level" || high.Value != "" {
		t.Fatalf("Level specs = %+v %+v", low, high)
	}

	if len(root.Consts) != 2 {
		t.Fatalf("package consts = %+v", root.Consts)
	}
	// go/doc sorts groups before single declarations.
	limits := root.Consts[0]
	if limits.Doc.Raw != "Limits of the sample.\n" || len(limits.Specs) != 2 || limits.Specs[1].Value != "1 << 3" {
		t.Fatalf("limits = %+v", limits)
	}
	dep := limits.Specs[1].Doc.Deprecated
	if dep == nil || dep.Spans[len(dep.Spans)-1].Kind != "text" || dep.Spans[len(dep.Spans)-2].Kind != "link" {
		t.Fatalf("OldB deprecation = %+v", dep)
	}
}

func TestDeterministic(t *testing.T) {
	a, err := Marshal(extractSample(t))
	if err != nil {
		t.Fatal(err)
	}
	b, err := Marshal(extractSample(t))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(a, b) {
		t.Fatal("two extractions differ")
	}
}
