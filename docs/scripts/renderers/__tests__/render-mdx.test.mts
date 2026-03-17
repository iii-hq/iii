import { describe, it, expect } from 'vitest'
import { renderSdkMdx } from '../render-mdx.mjs'
import type { SdkDoc } from '../../types.mjs'

function minimalDoc(overrides: Partial<SdkDoc> = {}): SdkDoc {
  return {
    metadata: {
      language: 'rust',
      languageLabel: 'Rust',
      title: 'Test SDK',
      description: 'Test.',
      installCommand: 'cargo add test',
      importExample: 'use test;',
    },
    initialization: {
      entryPoint: {
        name: 'init',
        signature: 'init()',
        description: 'Test init.',
        params: [],
        returns: { type: '', description: '' },
        examples: ['let x = 1;'],
      },
    },
    methods: [],
    types: [],
    ...overrides,
  }
}

// ---------------------------------------------------------------------------
// escapeMdxText - braces inside backtick code spans in descriptions
// ---------------------------------------------------------------------------

describe('renderSdkMdx - description brace escaping', () => {
  it('does not escape braces inside backtick code spans in method descriptions', () => {
    const doc = minimalDoc({
      methods: [
        {
          name: 'trigger',
          signature: 'trigger(request: TriggerRequest)',
          description: 'Call `trigger({ function_id, payload })` to invoke.',
          params: [],
          returns: { type: '', description: '' },
          examples: [],
        },
      ],
    })

    const mdx = renderSdkMdx(doc)
    expect(mdx).toContain('`trigger({ function_id, payload })`')
    expect(mdx).not.toContain('`trigger(\\{ function_id, payload \\})`')
  })

  it('still escapes braces in plain text (outside backticks)', () => {
    const doc = minimalDoc({
      methods: [
        {
          name: 'test',
          signature: 'test()',
          description: 'Returns {value} from the map.',
          params: [],
          returns: { type: '', description: '' },
          examples: [],
        },
      ],
    })

    const mdx = renderSdkMdx(doc)
    expect(mdx).toContain('Returns \\{value\\} from the map.')
  })

  it('does not escape braces inside backtick spans in type descriptions', () => {
    const doc = minimalDoc({
      types: [
        {
          name: 'TriggerRequest',
          description: 'Matches `trigger({ function_id })` signature.',
          fields: [],
        },
      ],
    })

    const mdx = renderSdkMdx(doc)
    expect(mdx).toContain('`trigger({ function_id })`')
    expect(mdx).not.toContain('\\{')
  })

  it('handles multiple backtick spans in one description', () => {
    const doc = minimalDoc({
      methods: [
        {
          name: 'test',
          signature: 'test()',
          description: 'Use `foo({a})` or `bar({b})` to call.',
          params: [],
          returns: { type: '', description: '' },
          examples: [],
        },
      ],
    })

    const mdx = renderSdkMdx(doc)
    expect(mdx).toContain('`foo({a})`')
    expect(mdx).toContain('`bar({b})`')
  })

  it('does not escape braces inside backtick spans in initialization description', () => {
    const doc = minimalDoc({
      initialization: {
        entryPoint: {
          name: 'init',
          signature: 'init()',
          description: 'Call `init({ key: "value" })` to start.',
          params: [],
          returns: { type: '', description: '' },
          examples: ['let x = 1;'],
        },
      },
    })

    const mdx = renderSdkMdx(doc)
    expect(mdx).toContain('`init({ key: "value" })`')
  })
})
