import { expectTypeOf, test } from 'vitest'
import type { HttpRequest, StreamingRequest, HttpResponse, StreamingResponse } from './types'

test('HttpRequest is buffered: has body, no request_body', () => {
  expectTypeOf<HttpRequest>().toHaveProperty('body')
  expectTypeOf<HttpRequest>().not.toHaveProperty('request_body')
})

test('StreamingRequest streams: has request_body, no body', () => {
  expectTypeOf<StreamingRequest>().toHaveProperty('request_body')
  expectTypeOf<StreamingRequest>().not.toHaveProperty('body')
})

test('HttpResponse is the buffered value shape', () => {
  expectTypeOf<HttpResponse>().toHaveProperty('status_code')
})

test('StreamingResponse is the handle', () => {
  expectTypeOf<StreamingResponse>().toHaveProperty('stream')
  expectTypeOf<StreamingResponse>().toHaveProperty('close')
})
