import assert from 'node:assert/strict'
import { test } from 'node:test'
import { rustTypeToString } from './parse-rustdoc.mjs'

const path = (name: string, args: any = null) => ({ resolved_path: { path: name, args } })
const bound = (name: string, args: any = null) => ({ trait_bound: { trait: { path: name, args } } })

for (const [name, output, expected] of [
  ['added', path('Result', { angle_bracketed: { args: [{ type: { tuple: [] } }, { type: path('ObserveRejection') }] } }), 'Result<(), ObserveRejection>'],
  ['removed', { tuple: [] }, '()'],
  ['deliver', path('Result', { angle_bracketed: { args: [{ type: path('Value') }, { type: path('Error') }] } }), 'Result<Value, Error>'],
] as const) {
  test(`preserves ${name} hook Future::Output, Send and borrowed lifetime`, () => {
    const future = {
      impl_trait: [
        bound('Future', { angle_bracketed: {
          args: [],
          constraints: [{ name: 'Output', args: null, binding: { equality: { type: output } } }],
        } }),
        bound('Send'),
        { outlives: "'_" },
      ],
    }
    assert.equal(rustTypeToString(future), `impl Future<Output = ${expected}> + Send + '_`)
  })
}

test('preserves positional type and lifetime arguments alongside associated types', () => {
  assert.equal(rustTypeToString({ impl_trait: [bound('Trait', { angle_bracketed: {
    args: [{ lifetime: "'a" }, { type: { primitive: 'u8' } }],
    constraints: [{ name: 'Item', binding: { equality: { type: { primitive: 'bool' } } } }],
  } })] }), "impl Trait<'a, u8, Item = bool>")
})
