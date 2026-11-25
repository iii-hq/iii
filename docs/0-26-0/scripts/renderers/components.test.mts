/**
 * Tests for components.mts. Run from the repo root:
 *   pnpm tsx --test docs/next/scripts/renderers/components.test.mts
 */
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { findBrokenTableRows, oneLineValue, renderValuesTable } from './components.mjs'

const engineSources = [
  'struct {',
  '\tFunctionsChanged     string',
  '\tWorkersChanged       string',
  '}{FunctionsChanged: FnOnFunctionsChanged, WorkersChanged: FnOnWorkersChanged}',
].join('\n')

test('oneLineValue puts a multi-line Go value on one line', () => {
  assert.equal(
    oneLineValue(engineSources),
    'struct { FunctionsChanged string; WorkersChanged string }{FunctionsChanged: FnOnFunctionsChanged, WorkersChanged: FnOnWorkersChanged}',
  )
})

test('oneLineValue leaves a one-line value alone', () => {
  assert.equal(oneLineValue('errors.New("iii: not connected")'), 'errors.New("iii: not connected")')
})

test('renderValuesTable keeps a multi-line value inside its row', () => {
  const table = renderValuesTable([{ name: 'EngineSources', value: engineSources, description: 'Engine source ids.' }])
  const lines = table.split('\n')
  assert.equal(lines.length, 3)
  assert.ok(lines[2]!.startsWith('| `EngineSources` | `struct { FunctionsChanged string;'))
  assert.deepEqual(findBrokenTableRows(table), [])
})

test('findBrokenTableRows reports a spilled cell and an unclosed code span', () => {
  const spilled = ['| Name | Value |', '| --- | --- |', '| `A` | `struct {', '\tB string', '}` |'].join('\n')
  assert.deepEqual(findBrokenTableRows(spilled), [
    'line 3: table row has an unclosed code span',
    'line 4: text right after a table row (a cell spilled onto the next line)',
  ])
})

test('findBrokenTableRows ignores fenced code and text after a blank line', () => {
  const ok = ['| A |', '| --- |', '| `x` |', '', 'Prose.', '```go', '| `unclosed |', '```'].join('\n')
  assert.deepEqual(findBrokenTableRows(ok), [])
})
