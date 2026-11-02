import { describe, expect, it } from 'vitest'
import * as nodeFilter from '../../iii/src/filter'
import { compileFilter, FilterError, FilterIndex } from '../src/filter'

// The browser SDK imports the Node SDK's evaluator instead of copying it, so
// the Node SDK's filter tests cover it.
describe('the shared evaluator', () => {
  it('is the Node SDK module, not a copy', () => {
    expect(compileFilter).toBe(nodeFilter.compileFilter)
    expect(FilterError).toBe(nodeFilter.FilterError)
    expect(FilterIndex).toBe(nodeFilter.FilterIndex)
  })
})
