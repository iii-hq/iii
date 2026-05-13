import { describe, it, expect } from 'vitest'
import { renderParamsTable, renderReturnsTable } from '../components.mjs'

// ---------------------------------------------------------------------------
// renderParamsTable - brace escaping inside code spans
// ---------------------------------------------------------------------------

describe('renderParamsTable', () => {
  it('does not escape braces inside backtick type cells', () => {
    const params = [
      {
        name: 'Enqueue',
        type: '{ queue: String }',
        description: 'Routes through a named queue.',
        required: true,
      },
    ]
    const result = renderParamsTable(params)
    expect(result).toContain('`{ queue: String }`')
    expect(result).not.toContain('\\{')
    expect(result).not.toContain('\\}')
  })

  it('does not escape braces in complex enum variant struct types', () => {
    const params = [
      {
        name: 'Remote',
        type: '{ code: String, message: String, stacktrace: Option<String> }',
        description: '-',
        required: true,
      },
    ]
    const result = renderParamsTable(params)
    expect(result).not.toContain('\\{')
    expect(result).not.toContain('\\}')
  })

  it('still escapes pipe characters in type cells for table integrity', () => {
    const params = [
      {
        name: 'value',
        type: 'A | B',
        description: '-',
        required: true,
      },
    ]
    const result = renderParamsTable(params)
    expect(result).toContain('\\|')
  })

  it('escapes braces in description text outside backticks', () => {
    const params = [
      {
        name: 'action',
        type: 'String',
        description: 'Use {value} here',
        required: true,
      },
    ]
    const result = renderParamsTable(params)
    expect(result).toContain('Use \\{value\\} here')
  })

  it('does not escape braces inside backtick spans in description text', () => {
    const params = [
      {
        name: 'otel',
        type: 'OtelConfig',
        description: 'Set `{ enabled: false }` to disable.',
        required: false,
      },
    ]
    const result = renderParamsTable(params)
    expect(result).toContain('`{ enabled: false }`')
    expect(result).not.toContain('`\\{')
  })

  it('correctly escapes braces outside backtick type links in mixed content', () => {
    const params = [
      {
        name: 'Enqueue',
        type: '{ queue: String }',
        description: '-',
        required: true,
      },
    ]
    const knownTypes = new Set(['String'])
    const result = renderParamsTable(params, knownTypes)
    // Braces in raw MDX between type links must be escaped
    expect(result).toContain('\\{')
    expect(result).toContain('[`String`](#string)')
  })
})

// ---------------------------------------------------------------------------
// renderReturnsTable - brace escaping
// ---------------------------------------------------------------------------

describe('renderReturnsTable', () => {
  it('does not escape braces in return type inside backtick code', () => {
    const returns = {
      type: 'Result<{ value: String }, Error>',
      description: 'The result.',
    }
    const result = renderReturnsTable(returns)
    expect(result).not.toContain('\\{')
    expect(result).not.toContain('\\}')
  })
})
