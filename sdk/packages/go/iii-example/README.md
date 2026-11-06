# iii Go SDK — examples

Runnable examples for the [iii Go SDK](../iii). This is a separate Go module that depends
on the sibling `iii` module via a `replace` directive (`../iii`), mirroring the Rust
`iii-example` crate and the Node `iii-example` package.

## Hello world

A worker that registers `hello::greet`, routes `POST /greet` to it by observing the http
worker's `http::on-request`, logs each greeting with an observer of its own, calls it once
over the socket, then serves until interrupted so the route can be called over HTTP.

```bash
# 1. Start an engine (in another terminal)
iii --use-default-config            # engine on ws://localhost:49134, HTTP on :3111

# 2. Run the worker
go run .                            # from sdk/packages/go/iii-example

# 3. Call the route with a JSON body, which the http worker parses into the request
curl -X POST localhost:3111/greet -H 'Content-Type: application/json' -d '{"name":"world"}'
# => {"message":"Hello, world!"}
```

Set `III_URL` to point at a non-default engine endpoint.
