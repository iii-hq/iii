#!/usr/bin/env lua
-- Lua client implemented on top of the grpcurl CLI.
-- Requires:
--   brew install grpcurl
--   luarocks install dkjson
--
-- Usage:
--   lua grpcurl_client.lua ../proto/engine.proto

local json = require("dkjson")

local proto = assert(arg[1], "usage: lua grpcurl_client.lua <path-to-engine.proto>")
local proto_dir = proto:match("^(.+)/[^/]+$") or "."
local target = os.getenv("ENGINE_ADDR") or "localhost:50051"
local base_cmd = string.format(
  "grpcurl --plaintext -import-path %s -proto %s",
  proto_dir,
  proto
)

local function run_grpc(method, payload)
  local tmp = os.tmpname()
  local fh = assert(io.open(tmp, "w"))
  fh:write(payload)
  fh:close()

  local cmd = string.format("%s -d @ %s engine.v1.Engine/%s < %s", base_cmd, target, method, tmp)
  local pipe = assert(io.popen(cmd .. " 2>&1"))
  local output = pipe:read("*a")
  local ok, reason, code = pipe:close()
  os.remove(tmp)

  if not ok then
    error(string.format("grpcurl call failed (%s): %s", tostring(code or reason), output))
  end

  local decoded, pos, err = json.decode(output, 1, nil)
  if not decoded then
    error("failed to parse grpcurl JSON: " .. tostring(err) .. "\nraw output: " .. output)
  end
  return decoded
end

local function call_unary()
  local payload = [[{
    "service": "text-formatter",
    "method": "format_text",
    "payload": "hello from lua",
    "meta": {"prefix": "[", "suffix": "]"}
  }]]
  local resp = run_grpc("Process", payload)
  print("Process response:", resp.result)
end

local function call_ruby()
  local payload = [[{
    "service": "ruby-reverser",
    "method": "reverse_text",
    "payload": "Lua client greets you",
    "meta": {"upcase": "true"}
  }]]
  local status, resp = pcall(run_grpc, "Process", payload)
  if status then
    print("Ruby worker response:", resp.result)
  else
    print("Ruby worker error:", resp)
  end
end

local function call_stream()
  local payload = [[{
    "service": "text-streamer",
    "method": "stream_chunks",
    "payload": "streaming hello from lua",
    "meta": {"chunk_size": "5", "delay_ms": "150"}
  }]]
  local cmd = string.format("%s -d '%s' %s engine.v1.Engine/StreamProcess", base_cmd, payload:gsub("'", "\\'"), target)
  local pipe = assert(io.popen(cmd .. " 2>&1"))
  print("StreamProcess response chunks:")
  for line in pipe:lines() do
    local decoded = json.decode(line)
    if decoded then
      print("  chunk:", decoded.result)
    else
      io.stderr:write(line .. "\n")
    end
  end
  pipe:close()
  print("Stream completed")
end

call_unary()
call_ruby()
call_stream()
