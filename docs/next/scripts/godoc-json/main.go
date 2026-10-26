// Command godoc-json dumps the go/doc documentation of every public package of a Go
// module as JSON. It is the Go step of the SDK API reference pipeline, the counterpart
// of the TypeDoc, griffe and rustdoc JSON dumps: docs/next/scripts/generate-api-docs.mts
// reads its output through parsers/parse-godoc.mts and renders
// docs/next/reference/sdk-go.mdx and helpers-go.mdx.
//
// Usage, from this directory:
//
//	go run . -root ../../../../sdk/packages/go/iii -out ../../../../sdk/packages/go/iii/api-docs.json
//
// It uses the standard library only, so it needs no module download. Every package of
// the module is dumped except main packages, nested modules and packages under
// internal/, testdata/, vendor/ or a directory whose name starts with "." or "_".
// Build constraints are evaluated for linux/amd64 whatever the host, and the output is
// deterministic: the same sources always give byte-identical JSON.
package main

import (
	"bufio"
	"bytes"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"go/ast"
	"go/build"
	"go/doc"
	"go/doc/comment"
	"go/format"
	"go/parser"
	"go/printer"
	"go/token"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

// Module is the whole dump: the module path and its documented packages, the root
// package first, then the others sorted by path.
type Module struct {
	Module    string    `json:"module"`
	GoVersion string    `json:"goVersion"`
	Packages  []Package `json:"packages"`
}

// Package is one package's exported API.
type Package struct {
	ImportPath string `json:"importPath"`
	// RelPath is the package directory relative to the module root, slash-separated;
	// empty for the root package.
	RelPath  string    `json:"relPath"`
	Name     string    `json:"name"`
	Synopsis string    `json:"synopsis"`
	Doc      Doc       `json:"doc"`
	Consts   []Value   `json:"consts"`
	Vars     []Value   `json:"vars"`
	Funcs    []Func    `json:"funcs"`
	Types    []Type    `json:"types"`
	Examples []Example `json:"examples"`
}

// Doc is a doc comment, parsed with go/doc/comment.
type Doc struct {
	// Raw is the comment text as go/doc returns it (markers such as
	// <!-- docs:internal --> are read from it).
	Raw    string  `json:"raw"`
	Blocks []Block `json:"blocks"`
	// Deprecated is the paragraph that starts with "Deprecated: " (the Go
	// convention), without that prefix. It is not repeated in Blocks.
	Deprecated *Block `json:"deprecated,omitempty"`
}

// Block is one block of a doc comment: "paragraph", "heading", "code" or "list".
type Block struct {
	Kind    string    `json:"kind"`
	Spans   []Span    `json:"spans,omitempty"`
	Code    string    `json:"code,omitempty"`
	Ordered bool      `json:"ordered,omitempty"`
	Items   [][]Block `json:"items,omitempty"`
}

// Span is inline text: "text" (plain, soft line breaks unwrapped), "doclink" (a
// [Name] doc link; Text is the name) or "link" (a URL).
type Span struct {
	Kind string `json:"kind"`
	Text string `json:"text"`
	URL  string `json:"url,omitempty"`
}

// Value is a const or var declaration group.
type Value struct {
	Doc   Doc         `json:"doc"`
	Specs []ValueSpec `json:"specs"`
}

// ValueSpec is one exported name of a Value. Value is the source expression; it is
// empty when the spec repeats the previous expression implicitly (iota).
type ValueSpec struct {
	Name  string `json:"name"`
	Type  string `json:"type,omitempty"`
	Value string `json:"value,omitempty"`
	Doc   Doc    `json:"doc"`
}

// Func is a function or a method.
type Func struct {
	Name string `json:"name"`
	// Recv is the receiver type of a method, e.g. "*Client".
	Recv string `json:"recv,omitempty"`
	// Signature is the declaration without body, on one line.
	Signature string `json:"signature"`
	// FuncType is the function type, e.g. "func(ctx context.Context) error".
	FuncType string    `json:"funcType"`
	Params   []Param   `json:"params"`
	Results  []Param   `json:"results"`
	Doc      Doc       `json:"doc"`
	Examples []Example `json:"examples"`
}

// Param is a parameter or result. Name is empty when unnamed.
type Param struct {
	Name     string `json:"name"`
	Type     string `json:"type"`
	Variadic bool   `json:"variadic,omitempty"`
}

// Type is a type declaration with what go/doc associates with it: typed constants
// and variables, constructor functions and methods.
type Type struct {
	Name string `json:"name"`
	// Kind is "struct", "interface", "alias", "func", "map", "slice", "array",
	// "chan" or "named" (a defined type over another named type).
	Kind string `json:"kind"`
	// Decl is the declaration on one line, e.g. "type Option func(*Client)"; empty
	// for structs and interfaces, whose members are listed instead.
	Decl string `json:"decl,omitempty"`
	Doc  Doc    `json:"doc"`
	// Fields are the exported struct fields, in source order.
	Fields []Field `json:"fields"`
	// InterfaceMethods are the methods (and embedded types) of an interface.
	InterfaceMethods []Field `json:"interfaceMethods"`
	// Incomplete is set when unexported fields or methods were left out.
	Incomplete bool      `json:"incomplete,omitempty"`
	Consts     []Value   `json:"consts"`
	Vars       []Value   `json:"vars"`
	Funcs      []Func    `json:"funcs"`
	Methods    []Func    `json:"methods"`
	Examples   []Example `json:"examples"`
}

// Field is a struct field or an interface method.
type Field struct {
	Name     string `json:"name"`
	Type     string `json:"type"`
	Embedded bool   `json:"embedded,omitempty"`
	// JSONTag is the field's json struct tag, when it has one.
	JSONTag *string `json:"jsonTag,omitempty"`
	Doc     Doc     `json:"doc"`
}

// Example is a testable example (an Example function in a _test.go file).
type Example struct {
	Suffix string `json:"suffix,omitempty"`
	Doc    string `json:"doc,omitempty"`
	Code   string `json:"code"`
}

func main() {
	root := flag.String("root", ".", "directory of the Go module to document (the one holding go.mod)")
	out := flag.String("out", "", "output file (default: standard output)")
	flag.Parse()

	m, err := Extract(*root)
	if err != nil {
		fmt.Fprintln(os.Stderr, "godoc-json:", err)
		os.Exit(1)
	}
	data, err := Marshal(m)
	if err != nil {
		fmt.Fprintln(os.Stderr, "godoc-json:", err)
		os.Exit(1)
	}
	if *out == "" {
		_, err = os.Stdout.Write(data)
	} else {
		err = os.WriteFile(*out, data, 0o644)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "godoc-json:", err)
		os.Exit(1)
	}
	if *out != "" {
		fmt.Fprintf(os.Stderr, "godoc-json: wrote %s (%d packages)\n", *out, len(m.Packages))
	}
}

// Marshal encodes the dump as indented JSON without HTML escaping.
func Marshal(m *Module) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	enc.SetIndent("", "  ")
	if err := enc.Encode(m); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

// Extract documents the module rooted at root.
func Extract(root string) (*Module, error) {
	root, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	modPath, goVersion, err := readGoMod(filepath.Join(root, "go.mod"))
	if err != nil {
		return nil, err
	}

	ctx := build.Default
	ctx.GOOS, ctx.GOARCH = "linux", "amd64"
	ctx.CgoEnabled = false
	ctx.BuildTags = nil

	m := &Module{Module: modPath, GoVersion: goVersion, Packages: []Package{}}
	err = filepath.WalkDir(root, func(dir string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if !d.IsDir() {
			return nil
		}
		if dir != root {
			switch name := d.Name(); {
			case name == "internal", name == "testdata", name == "vendor",
				strings.HasPrefix(name, "."), strings.HasPrefix(name, "_"):
				return filepath.SkipDir
			}
			if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
				return filepath.SkipDir // a nested module documents itself
			}
		}
		pkg, err := extractDir(&ctx, root, dir, modPath)
		if err != nil {
			return err
		}
		if pkg != nil {
			m.Packages = append(m.Packages, *pkg)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.SliceStable(m.Packages, func(i, j int) bool { return m.Packages[i].RelPath < m.Packages[j].RelPath })
	return m, nil
}

// readGoMod returns the module path and the go directive of a go.mod file.
func readGoMod(file string) (modPath, goVersion string, err error) {
	f, err := os.Open(file)
	if err != nil {
		return "", "", err
	}
	defer f.Close()
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		fields := strings.Fields(sc.Text())
		if len(fields) < 2 {
			continue
		}
		switch fields[0] {
		case "module":
			modPath = strings.Trim(fields[1], `"`)
		case "go":
			goVersion = fields[1]
		}
	}
	if err := sc.Err(); err != nil {
		return "", "", err
	}
	if modPath == "" {
		return "", "", fmt.Errorf("%s: no module directive", file)
	}
	return modPath, goVersion, nil
}

type extractor struct {
	fset   *token.FileSet
	parser *comment.Parser
}

// extractDir documents the package in dir, or returns nil when there is none to
// document (no Go files, or a main package).
func extractDir(ctx *build.Context, root, dir, modPath string) (*Package, error) {
	bp, err := ctx.ImportDir(dir, 0)
	if err != nil {
		var noGo *build.NoGoError
		if errors.As(err, &noGo) {
			return nil, nil
		}
		return nil, fmt.Errorf("%s: %w", dir, err)
	}
	if bp.Name == "main" {
		return nil, nil
	}
	rel, err := filepath.Rel(root, dir)
	if err != nil {
		return nil, err
	}
	rel = filepath.ToSlash(rel)
	if rel == "." {
		rel = ""
	}
	importPath := modPath
	if rel != "" {
		importPath += "/" + rel
	}

	fset := token.NewFileSet()
	var files []*ast.File
	// Test files only contribute their Example functions.
	for _, names := range [][]string{bp.GoFiles, bp.TestGoFiles, bp.XTestGoFiles} {
		for _, name := range names {
			f, err := parser.ParseFile(fset, filepath.Join(dir, name), nil, parser.ParseComments)
			if err != nil {
				return nil, err
			}
			files = append(files, f)
		}
	}
	dp, err := doc.NewFromFiles(fset, files, importPath)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", dir, err)
	}

	x := &extractor{fset: fset, parser: dp.Parser()}
	pkg := &Package{
		ImportPath: importPath,
		RelPath:    rel,
		Name:       dp.Name,
		Synopsis:   dp.Synopsis(dp.Doc),
		Doc:        x.doc(dp.Doc),
		Consts:     x.values(dp.Consts),
		Vars:       x.values(dp.Vars),
		Funcs:      x.funcs(dp.Funcs),
		Types:      []Type{},
		Examples:   x.examples(dp.Examples),
	}
	for _, t := range dp.Types {
		pkg.Types = append(pkg.Types, x.typ(t))
	}
	return pkg, nil
}

// --- declarations ---------------------------------------------------------

func (x *extractor) values(vs []*doc.Value) []Value {
	out := []Value{}
	for _, v := range vs {
		val := Value{Doc: x.doc(v.Doc), Specs: []ValueSpec{}}
		for _, s := range v.Decl.Specs {
			spec, ok := s.(*ast.ValueSpec)
			if !ok {
				continue
			}
			text := spec.Doc.Text()
			if text == "" {
				text = spec.Comment.Text()
			}
			specDoc := x.doc(text)
			typ := ""
			if spec.Type != nil {
				typ = x.expr(spec.Type)
			}
			for i, name := range spec.Names {
				// go/doc turns unexported names that must keep their place into "_".
				if !name.IsExported() {
					continue
				}
				item := ValueSpec{Name: name.Name, Type: typ, Doc: specDoc}
				if i < len(spec.Values) {
					item.Value = x.expr(spec.Values[i])
				}
				val.Specs = append(val.Specs, item)
			}
		}
		if len(val.Specs) > 0 {
			out = append(out, val)
		}
	}
	return out
}

func (x *extractor) funcs(list []*doc.Func) []Func {
	out := []Func{}
	for _, f := range list {
		decl := *f.Decl
		decl.Doc, decl.Body = nil, nil
		out = append(out, Func{
			Name:      f.Name,
			Recv:      f.Recv,
			Signature: x.print(&decl),
			FuncType:  x.expr(f.Decl.Type),
			Params:    x.params(f.Decl.Type.Params),
			Results:   x.params(f.Decl.Type.Results),
			Doc:       x.doc(f.Doc),
			Examples:  x.examples(f.Examples),
		})
	}
	return out
}

func (x *extractor) params(fl *ast.FieldList) []Param {
	out := []Param{}
	if fl == nil {
		return out
	}
	for _, field := range fl.List {
		typ := x.expr(field.Type)
		_, variadic := field.Type.(*ast.Ellipsis)
		if len(field.Names) == 0 {
			out = append(out, Param{Type: typ, Variadic: variadic})
			continue
		}
		for _, name := range field.Names {
			out = append(out, Param{Name: name.Name, Type: typ, Variadic: variadic})
		}
	}
	return out
}

func (x *extractor) typ(t *doc.Type) Type {
	out := Type{
		Name:             t.Name,
		Kind:             "named",
		Doc:              x.doc(t.Doc),
		Fields:           []Field{},
		InterfaceMethods: []Field{},
		Consts:           x.values(t.Consts),
		Vars:             x.values(t.Vars),
		Funcs:            x.funcs(t.Funcs),
		Methods:          x.funcs(t.Methods),
		Examples:         x.examples(t.Examples),
	}
	var spec *ast.TypeSpec
	for _, s := range t.Decl.Specs {
		if ts, ok := s.(*ast.TypeSpec); ok && ts.Name.Name == t.Name {
			spec = ts
			break
		}
	}
	if spec == nil {
		return out
	}
	switch tt := spec.Type.(type) {
	case *ast.StructType:
		out.Kind = "struct"
		out.Fields = x.fields(tt.Fields, true)
		out.Incomplete = tt.Incomplete
	case *ast.InterfaceType:
		out.Kind = "interface"
		out.InterfaceMethods = x.fields(tt.Methods, false)
		out.Incomplete = tt.Incomplete
	default:
		switch {
		case spec.Assign.IsValid():
			out.Kind = "alias"
		default:
			out.Kind = exprKind(spec.Type)
		}
		declSpec := *spec
		declSpec.Doc, declSpec.Comment = nil, nil
		decl := withoutPos(reflect.ValueOf(&declSpec)).Interface().(*ast.TypeSpec)
		if spec.Assign.IsValid() {
			decl.Assign = 1 // the printer reads an alias from a valid Assign position
		}
		out.Decl = "type " + x.printRaw(decl)
	}
	return out
}

func exprKind(e ast.Expr) string {
	switch e := e.(type) {
	case *ast.FuncType:
		return "func"
	case *ast.MapType:
		return "map"
	case *ast.ArrayType:
		if e.Len == nil {
			return "slice"
		}
		return "array"
	case *ast.ChanType:
		return "chan"
	case *ast.ParenExpr:
		return exprKind(e.X)
	}
	return "named"
}

// fields lists struct fields (isStruct true) or interface methods. go/doc has already
// dropped the unexported ones.
func (x *extractor) fields(fl *ast.FieldList, isStruct bool) []Field {
	out := []Field{}
	if fl == nil {
		return out
	}
	for _, f := range fl.List {
		text := f.Doc.Text()
		if text == "" {
			text = f.Comment.Text()
		}
		fd := x.doc(text)
		typ := x.expr(f.Type)
		var jsonTag *string
		if isStruct && f.Tag != nil {
			if tag, err := strconv.Unquote(f.Tag.Value); err == nil {
				if v, ok := reflect.StructTag(tag).Lookup("json"); ok {
					jsonTag = &v
				}
			}
		}
		if len(f.Names) == 0 {
			name := embeddedName(f.Type)
			if isStruct && !token.IsExported(name) {
				continue
			}
			out = append(out, Field{Name: name, Type: typ, Embedded: true, JSONTag: jsonTag, Doc: fd})
			continue
		}
		for _, name := range f.Names {
			if !name.IsExported() {
				continue
			}
			out = append(out, Field{Name: name.Name, Type: typ, JSONTag: jsonTag, Doc: fd})
		}
	}
	return out
}

// embeddedName is the field name of an embedded type: its type name without
// pointer, package qualifier or type arguments.
func embeddedName(e ast.Expr) string {
	switch e := e.(type) {
	case *ast.Ident:
		return e.Name
	case *ast.StarExpr:
		return embeddedName(e.X)
	case *ast.SelectorExpr:
		return e.Sel.Name
	case *ast.IndexExpr:
		return embeddedName(e.X)
	case *ast.IndexListExpr:
		return embeddedName(e.X)
	}
	return ""
}

// --- examples -------------------------------------------------------------

var outputCommentRE = regexp.MustCompile(`(?m)^[ \t]*// (?:Unordered output|Output):`)

func (x *extractor) examples(exs []*doc.Example) []Example {
	out := []Example{}
	for _, ex := range exs {
		var buf bytes.Buffer
		if err := format.Node(&buf, x.fset, &printer.CommentedNode{Node: ex.Code, Comments: ex.Comments}); err != nil {
			continue
		}
		code := buf.String()
		if _, ok := ex.Code.(*ast.BlockStmt); ok {
			code = unwrapBlock(code)
		}
		if loc := outputCommentRE.FindStringIndex(code); loc != nil {
			code = code[:loc[0]]
		}
		out = append(out, Example{
			Suffix: ex.Suffix,
			Doc:    strings.TrimSpace(ex.Doc),
			Code:   strings.Trim(code, "\n"),
		})
	}
	return out
}

// unwrapBlock turns a printed function body "{\n\tstmt\n}" into its statements,
// one indentation level less.
func unwrapBlock(code string) string {
	code = strings.TrimSpace(code)
	code = strings.TrimSuffix(strings.TrimPrefix(code, "{"), "}")
	lines := strings.Split(strings.Trim(code, "\n"), "\n")
	for i, line := range lines {
		lines[i] = strings.TrimPrefix(line, "\t")
	}
	return strings.Join(lines, "\n")
}

// --- doc comments ---------------------------------------------------------

var deprecatedRE = regexp.MustCompile(`^Deprecated:\s*`)

func (x *extractor) doc(text string) Doc {
	d := Doc{Raw: text, Blocks: []Block{}}
	if strings.TrimSpace(text) == "" {
		return d
	}
	for _, b := range x.parser.Parse(text).Content {
		blk := x.block(b)
		if d.Deprecated == nil && blk.Kind == "paragraph" && len(blk.Spans) > 0 &&
			blk.Spans[0].Kind == "text" && deprecatedRE.MatchString(blk.Spans[0].Text) {
			blk.Spans[0].Text = deprecatedRE.ReplaceAllString(blk.Spans[0].Text, "")
			if blk.Spans[0].Text == "" {
				blk.Spans = blk.Spans[1:]
			}
			d.Deprecated = &blk
			continue
		}
		d.Blocks = append(d.Blocks, blk)
	}
	return d
}

func (x *extractor) block(b comment.Block) Block {
	switch b := b.(type) {
	case *comment.Paragraph:
		return Block{Kind: "paragraph", Spans: spans(b.Text)}
	case *comment.Heading:
		return Block{Kind: "heading", Spans: spans(b.Text)}
	case *comment.Code:
		return Block{Kind: "code", Code: strings.TrimRight(b.Text, "\n")}
	case *comment.List:
		blk := Block{Kind: "list", Ordered: len(b.Items) > 0 && b.Items[0].Number != "", Items: [][]Block{}}
		for _, item := range b.Items {
			content := []Block{}
			for _, c := range item.Content {
				content = append(content, x.block(c))
			}
			blk.Items = append(blk.Items, content)
		}
		return blk
	}
	return Block{Kind: "paragraph", Spans: []Span{}}
}

// spans converts inline text, unwrapping soft line breaks and merging adjacent plain
// runs.
func spans(texts []comment.Text) []Span {
	out := []Span{}
	add := func(s Span) {
		if s.Kind == "text" && len(out) > 0 && out[len(out)-1].Kind == "text" {
			out[len(out)-1].Text += s.Text
			return
		}
		out = append(out, s)
	}
	for _, t := range texts {
		switch t := t.(type) {
		case comment.Plain:
			add(Span{Kind: "text", Text: unwrap(string(t))})
		case comment.Italic:
			add(Span{Kind: "text", Text: unwrap(string(t))})
		case *comment.Link:
			add(Span{Kind: "link", Text: unwrap(plain(t.Text)), URL: t.URL})
		case *comment.DocLink:
			add(Span{Kind: "doclink", Text: unwrap(plain(t.Text))})
		}
	}
	if len(out) > 0 {
		if out[0].Kind == "text" {
			out[0].Text = strings.TrimLeft(out[0].Text, " ")
		}
		if last := len(out) - 1; out[last].Kind == "text" {
			out[last].Text = strings.TrimRight(out[last].Text, " ")
		}
	}
	return out
}

func plain(texts []comment.Text) string {
	var b strings.Builder
	for _, t := range texts {
		switch t := t.(type) {
		case comment.Plain:
			b.WriteString(string(t))
		case comment.Italic:
			b.WriteString(string(t))
		case *comment.Link:
			b.WriteString(plain(t.Text))
		case *comment.DocLink:
			b.WriteString(plain(t.Text))
		}
	}
	return b.String()
}

func unwrap(s string) string { return strings.ReplaceAll(s, "\n", " ") }

// --- printing -------------------------------------------------------------

// print formats a node on one line: positions are dropped first, so the printer
// keeps nothing of the source layout ("struct{ A int; B int }", one-line
// signatures and composite literals).
func (x *extractor) print(n any) string {
	return x.printRaw(withoutPos(reflect.ValueOf(n)).Interface())
}

// printRaw formats a node as it is, positions included.
func (x *extractor) printRaw(n any) string {
	var buf bytes.Buffer
	if err := format.Node(&buf, x.fset, n); err != nil {
		return ""
	}
	return strings.TrimSpace(buf.String())
}

func (x *extractor) expr(e ast.Expr) string { return x.print(e) }

var (
	posType    = reflect.TypeOf(token.NoPos)
	objectType = reflect.TypeOf((*ast.Object)(nil))
	scopeType  = reflect.TypeOf((*ast.Scope)(nil))
)

// withoutPos deep-copies an AST value with every token.Pos zeroed. Identifier
// resolution links (ast.Object, ast.Scope) are dropped: they form cycles and the
// printer does not read them.
func withoutPos(v reflect.Value) reflect.Value {
	switch v.Kind() {
	case reflect.Pointer:
		if v.IsNil() || v.Type() == objectType || v.Type() == scopeType {
			return reflect.Zero(v.Type())
		}
		c := reflect.New(v.Elem().Type())
		c.Elem().Set(withoutPos(v.Elem()))
		return c
	case reflect.Interface:
		if v.IsNil() {
			return v
		}
		c := reflect.New(v.Type()).Elem()
		c.Set(withoutPos(v.Elem()))
		return c
	case reflect.Struct:
		c := reflect.New(v.Type()).Elem()
		for i := 0; i < v.NumField(); i++ {
			if !c.Field(i).CanSet() || v.Field(i).Type() == posType {
				continue
			}
			c.Field(i).Set(withoutPos(v.Field(i)))
		}
		return c
	case reflect.Slice:
		if v.IsNil() {
			return v
		}
		c := reflect.MakeSlice(v.Type(), v.Len(), v.Len())
		for i := 0; i < v.Len(); i++ {
			c.Index(i).Set(withoutPos(v.Index(i)))
		}
		return c
	}
	return v
}
