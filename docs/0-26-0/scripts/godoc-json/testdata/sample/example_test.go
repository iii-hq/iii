package sample_test

import "example.com/sample"

// Example shows the package.
func Example() {
	c := sample.New(sample.DefaultURL)
	_ = c.Send(nil)
	// Output:
}

func ExampleClient_Send() {
	c := sample.New("")
	// send nothing
	_ = c.Send(nil)
}
