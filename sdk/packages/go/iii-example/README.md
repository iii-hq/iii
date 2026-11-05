# iii Go SDK — examples

Runnable examples for the [iii Go SDK](../iii). This is a separate Go module that depends
on the sibling `iii` module via a `replace` directive (`../iii`), mirroring the Rust
`iii-example` crate and the Node `iii-example` package.

## Hello world

A worker that registers `hello::greet`, binds an HTTP trigger to it, invokes it once over
the socket, then serves until interrupted so the trigger can be called over HTTP.

```bash
# 1. Start the engine in another terminal
iii                                    # SDK socket on ws://localhost:49134

# 2. From this directory, start the Compose daemon in a third terminal and add the
#    http worker, which serves :3111. The included worker-compose.yaml connects the
#    daemon to the engine; the engine itself only speaks WebSocket.
iii compose
iii trigger compose::add worker=http

# 3. Run the worker
go run .                               # from sdk/packages/go/iii-example

# 4. Call the HTTP trigger. The Content-Type header matters: the http worker only parses
#    a JSON body into the request envelope, and curl's -d default is form-urlencoded.
curl -X POST localhost:3111/greet -H 'Content-Type: application/json' -d '{"name":"world"}'
# => {"message":"Hello, world!"}
```

`http` is a Compose worker, not part of the engine, so step 2 adds it explicitly. Set `III_URL`
and update `worker-compose.yaml` to point at a non-default engine endpoint.
