// Package filterlink carries the equality conditions of a compiled filter from package
// filter to package iii, which indexes the observers of each function it hosts by
// them. It is internal: the filter package's API is what it documents.
package filterlink

// Kinds of [Scalar].
const (
	Null uint8 = iota
	Bool
	Number
	String
)

// Scalar is a value an equality condition names, as the filter language compares it: a
// number by its value as a double (Num, with -0 as 0), a string by its bytes (Str), a
// boolean as Num 0 or 1, and null as itself. Two Scalars are equal exactly when the
// language says their values are, so a Scalar is a map key.
type Scalar struct {
	Kind uint8
	Num  float64
	Str  string
}

// Path is a document path an equality condition reads.
type Path struct {
	// Key identifies the path: two paths with the same key read the same value of
	// every document.
	Key string
	// Resolve reads the path from doc, a filter.Doc, as the evaluator does: its value
	// as a Scalar, and false when the path is absent or holds an array or an object
	// (which equal no scalar).
	Resolve func(doc any) (Scalar, bool)
}

// Term is an equality condition of a filter's top-level $and: the document's value at
// Path is one of Values. A document whose value at Path equals none of them does not
// match the filter.
type Term struct {
	Path *Path
	// Values are the scalars the condition names, each once, at least one.
	Values []Scalar
	// In is set for an $in, which may name several values; an $eq names one.
	In bool
}

// Terms returns the equality conditions of f, a *filter.Filter: for each path clause of
// its top-level $and (an explicit $and adds the clauses of its filters), its $eq of a
// scalar, and its $in when every item is a scalar. Package filter sets it.
var Terms func(f any) []Term
