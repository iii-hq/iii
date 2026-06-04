// Module for the iii Go SDK examples. It depends on the sibling iii module via a
// replace directive pointing at ../iii, so the examples build against the local SDK
// source (there is no published module tag yet). Mirrors how the Rust iii-example uses
// `iii-sdk = { path = "../iii" }` and the Node one uses `iii-sdk: workspace:*`.
module github.com/iii-hq/iii/sdk/packages/go/iii-example

go 1.24

require github.com/iii-hq/iii/sdk/packages/go/iii v0.0.0

require (
	github.com/bahlo/generic-list-go v0.2.0 // indirect
	github.com/buger/jsonparser v1.1.2 // indirect
	github.com/coder/websocket v1.8.14 // indirect
	github.com/google/uuid v1.6.0 // indirect
	github.com/invopop/jsonschema v0.14.0 // indirect
	github.com/pb33f/ordered-map/v2 v2.3.1 // indirect
	go.yaml.in/yaml/v4 v4.0.0-rc.2 // indirect
)

replace github.com/iii-hq/iii/sdk/packages/go/iii => ../iii
