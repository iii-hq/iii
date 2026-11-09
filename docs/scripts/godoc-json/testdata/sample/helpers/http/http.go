// Package http holds HTTP helpers.
//
// Import it under an alias to keep net/http available:
//
//	import samplehttp "example.com/sample/helpers/http"
package http

// Method is an HTTP method.
type Method string

// Methods.
const (
	Get  Method = "GET"
	Post Method = "POST"
)
