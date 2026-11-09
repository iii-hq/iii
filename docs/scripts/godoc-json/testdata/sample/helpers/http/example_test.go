package http_test

import (
	"fmt"

	shttp "example.com/sample/helpers/http"
)

// The doc's alias wins over this one.
func ExampleMethod() {
	fmt.Println(shttp.Get)
}
