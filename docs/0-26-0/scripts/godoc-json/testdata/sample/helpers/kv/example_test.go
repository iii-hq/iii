package kv_test

import kvx "example.com/sample/helpers/kv"

func ExampleGet() {
	_ = kvx.Get("a")
}
