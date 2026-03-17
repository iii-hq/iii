import { describe, it, expect } from 'vitest'
import {
  annotationToString,
  extractDocstring,
  extractParams,
  extractExamples,
  isAsync,
  buildSignature,
  griffeToFunction,
  extractAttributeDescriptions,
  griffeToType,
  extractTypesFromModule,
  parseGriffeData,
} from '../parse-griffe.mjs'

// ---------------------------------------------------------------------------
// Helpers for building mock griffe objects
// ---------------------------------------------------------------------------

function makeObj(
  overrides: Partial<{
    name: string
    kind: string
    docstring: { value?: string; parsed?: any[] }
    members: Record<string, any>
    parameters: { name: string; annotation?: any; default?: string | null }[]
    returns: { annotation?: any }
    labels: string[]
    annotation: any
    value: string | null
  }> = {},
) {
  return {
    name: overrides.name ?? 'test_func',
    kind: overrides.kind ?? 'function',
    ...overrides,
  }
}

// ---------------------------------------------------------------------------
// annotationToString
// ---------------------------------------------------------------------------

describe('annotationToString', () => {
  it('returns empty string for null/undefined', () => {
    expect(annotationToString(null)).toBe('')
    expect(annotationToString(undefined)).toBe('')
  })

  it('returns the string itself when given a string', () => {
    expect(annotationToString('str')).toBe('str')
    expect(annotationToString('int')).toBe('int')
  })

  it('handles ExprName', () => {
    expect(annotationToString({ cls: 'ExprName', name: 'str' })).toBe('str')
  })

  it('handles ExprName with missing name', () => {
    expect(annotationToString({ cls: 'ExprName' })).toBe('')
  })

  it('handles ExprBinOp (union type)', () => {
    expect(
      annotationToString({
        cls: 'ExprBinOp',
        left: { cls: 'ExprName', name: 'str' },
        right: { cls: 'ExprName', name: 'None' },
        operator: '|',
      }),
    ).toBe('str | None')
  })

  it('handles ExprBinOp with default operator', () => {
    expect(
      annotationToString({
        cls: 'ExprBinOp',
        left: { cls: 'ExprName', name: 'str' },
        right: { cls: 'ExprName', name: 'int' },
      }),
    ).toBe('str | int')
  })

  it('handles ExprBinOp with empty left', () => {
    expect(
      annotationToString({
        cls: 'ExprBinOp',
        left: null,
        right: { cls: 'ExprName', name: 'int' },
      }),
    ).toBe('int')
  })

  it('handles ExprBinOp with empty right', () => {
    expect(
      annotationToString({
        cls: 'ExprBinOp',
        left: { cls: 'ExprName', name: 'str' },
        right: null,
      }),
    ).toBe('str')
  })

  it('handles ExprBinOp with both sides empty', () => {
    expect(
      annotationToString({
        cls: 'ExprBinOp',
        left: null,
        right: null,
      }),
    ).toBe('')
  })

  it('handles ExprSubscript (generic type)', () => {
    expect(
      annotationToString({
        cls: 'ExprSubscript',
        left: { cls: 'ExprName', name: 'Optional' },
        slice: { cls: 'ExprName', name: 'str' },
      }),
    ).toBe('Optional[str]')
  })

  it('handles ExprSubscript with no slice', () => {
    expect(
      annotationToString({
        cls: 'ExprSubscript',
        left: { cls: 'ExprName', name: 'List' },
        slice: null,
      }),
    ).toBe('List')
  })

  it('handles ExprSubscript with empty base', () => {
    expect(
      annotationToString({
        cls: 'ExprSubscript',
        left: null,
        slice: { cls: 'ExprName', name: 'str' },
      }),
    ).toBe('')
  })

  it('handles ExprTuple', () => {
    expect(
      annotationToString({
        cls: 'ExprTuple',
        elements: [
          { cls: 'ExprName', name: 'str' },
          { cls: 'ExprName', name: 'int' },
        ],
      }),
    ).toBe('str, int')
  })

  it('handles ExprTuple with empty/missing elements', () => {
    expect(annotationToString({ cls: 'ExprTuple', elements: [] })).toBe('')
    expect(annotationToString({ cls: 'ExprTuple' })).toBe('')
  })

  it('handles ExprAttribute', () => {
    expect(annotationToString({ cls: 'ExprAttribute', member: 'MyType' })).toBe('MyType')
  })

  it('handles ExprAttribute falling back to name', () => {
    expect(annotationToString({ cls: 'ExprAttribute', name: 'Fallback' })).toBe('Fallback')
  })

  it('handles unknown cls with source fallback', () => {
    expect(annotationToString({ cls: 'UnknownExpr', source: 'Dict[str, Any]' })).toBe('Dict[str, Any]')
  })

  it('handles unknown cls without source', () => {
    expect(annotationToString({ cls: 'UnknownExpr' })).toBe('')
  })
})

// ---------------------------------------------------------------------------
// extractDocstring
// ---------------------------------------------------------------------------

describe('extractDocstring', () => {
  it('returns empty string when no docstring', () => {
    expect(extractDocstring(makeObj())).toBe('')
  })

  it('returns empty string when docstring value is empty', () => {
    expect(extractDocstring(makeObj({ docstring: { value: '' } }))).toBe('')
  })

  it('returns raw docstring value when no parsed sections', () => {
    expect(
      extractDocstring(makeObj({ docstring: { value: 'A simple description.' } })),
    ).toBe('A simple description.')
  })

  it('strips Args: section from raw docstring', () => {
    expect(
      extractDocstring(makeObj({ docstring: { value: 'Main description.\n\nArgs:\n    x: the value' } })),
    ).toBe('Main description.')
  })

  it('strips Attributes: section from raw docstring', () => {
    expect(
      extractDocstring(makeObj({ docstring: { value: 'Main description.\n\nAttributes:\n    name: the name' } })),
    ).toBe('Main description.')
  })

  it('strips Returns: section from raw docstring', () => {
    expect(
      extractDocstring(makeObj({ docstring: { value: 'Main description.\n\nReturns:\n    The result' } })),
    ).toBe('Main description.')
  })

  it('strips Raises: section from raw docstring', () => {
    expect(
      extractDocstring(makeObj({ docstring: { value: 'Main description.\n\nRaises:\n    ValueError: if invalid' } })),
    ).toBe('Main description.')
  })

  it('strips Examples: section from raw docstring', () => {
    expect(
      extractDocstring(makeObj({ docstring: { value: 'Main description.\n\nExamples:\n    >>> foo()' } })),
    ).toBe('Main description.')
  })

  it('strips singular Example: section from raw docstring', () => {
    expect(
      extractDocstring(makeObj({ docstring: { value: 'Main description.\n\nExample:\n    >>> foo()' } })),
    ).toBe('Main description.')
  })

  it('strips Note: section from raw docstring', () => {
    expect(
      extractDocstring(makeObj({ docstring: { value: 'Main description.\n\nNote:\n    Something important' } })),
    ).toBe('Main description.')
  })

  it('strips Yields: section from raw docstring', () => {
    expect(
      extractDocstring(makeObj({ docstring: { value: 'Main description.\n\nYields:\n    Items one by one' } })),
    ).toBe('Main description.')
  })

  it('strips See Also: section from raw docstring', () => {
    expect(
      extractDocstring(makeObj({ docstring: { value: 'Main description.\n\nSee Also:\n    other_func' } })),
    ).toBe('Main description.')
  })

  it('strips multiple Google-style sections at once', () => {
    const doc = 'Main description.\n\nArgs:\n    x: val\n\nReturns:\n    result\n\nRaises:\n    Error'
    expect(extractDocstring(makeObj({ docstring: { value: doc } }))).toBe('Main description.')
  })

  it('uses parsed text sections when available', () => {
    const parsed = [
      { kind: 'text', value: 'First paragraph.' },
      { kind: 'parameters', value: [{ name: 'x', description: 'a param' }] },
      { kind: 'text', value: 'Second paragraph.' },
    ]
    expect(
      extractDocstring(makeObj({ docstring: { value: 'raw value', parsed } })),
    ).toBe('First paragraph.\nSecond paragraph.')
  })

  it('skips non-string values in parsed text sections', () => {
    const parsed = [
      { kind: 'text', value: 'Description.' },
      { kind: 'text', value: [{ name: 'not a string' }] },
    ]
    expect(
      extractDocstring(makeObj({ docstring: { value: '', parsed } })),
    ).toBe('Description.')
  })
})

// ---------------------------------------------------------------------------
// extractParams
// ---------------------------------------------------------------------------

describe('extractParams', () => {
  it('returns empty array when no parameters', () => {
    expect(extractParams(makeObj())).toEqual([])
  })

  it('extracts params from parsed docstring sections', () => {
    const result = extractParams(makeObj({
      parameters: [
        { name: 'self' },
        { name: 'address', annotation: { cls: 'ExprName', name: 'str' } },
        { name: 'port', annotation: { cls: 'ExprName', name: 'int' }, default: '8080' },
      ],
      docstring: {
        value: '',
        parsed: [
          {
            kind: 'parameters',
            value: [
              { name: 'address', description: 'The server address' },
              { name: 'port', description: 'The port number' },
            ],
          },
        ],
      },
    }))

    expect(result).toEqual([
      { name: 'address', type: 'str', description: 'The server address', required: true },
      { name: 'port', type: 'int', description: 'The port number', required: false },
    ])
  })

  it('filters out self and cls parameters', () => {
    const result = extractParams(makeObj({
      parameters: [
        { name: 'self' },
        { name: 'cls' },
        { name: 'value', annotation: { cls: 'ExprName', name: 'str' } },
      ],
    }))

    expect(result).toHaveLength(1)
    expect(result[0].name).toBe('value')
  })

  it('falls back to Args: section in raw docstring', () => {
    const result = extractParams(makeObj({
      parameters: [
        { name: 'name', annotation: { cls: 'ExprName', name: 'str' } },
        { name: 'age', annotation: { cls: 'ExprName', name: 'int' } },
      ],
      docstring: {
        value: 'A function.\n\nArgs:\n    name: The user name\n    age: The user age',
      },
    }))

    expect(result).toEqual([
      { name: 'name', type: 'str', description: 'The user name', required: true },
      { name: 'age', type: 'int', description: 'The user age', required: true },
    ])
  })

  it('handles multi-line arg descriptions in raw docstring', () => {
    const result = extractParams(makeObj({
      parameters: [{ name: 'data', annotation: { cls: 'ExprName', name: 'dict' } }],
      docstring: {
        value: 'A function.\n\nArgs:\n    data: The data payload\n        that spans multiple lines',
      },
    }))

    expect(result[0].description).toBe('The data payload that spans multiple lines')
  })

  it('stops parsing Args at Returns: section', () => {
    const result = extractParams(makeObj({
      parameters: [{ name: 'x', annotation: { cls: 'ExprName', name: 'int' } }],
      docstring: {
        value: 'Func.\n\nArgs:\n    x: a number\n\nReturns:\n    The result',
      },
    }))

    expect(result[0].description).toBe('a number')
  })

  it('defaults type to Any when no annotation', () => {
    const result = extractParams(makeObj({ parameters: [{ name: 'value' }] }))
    expect(result[0].type).toBe('Any')
  })

  it('marks params with default as not required', () => {
    const result = extractParams(makeObj({
      parameters: [
        { name: 'required_param' },
        { name: 'optional_param', default: 'None' },
      ],
    }))

    expect(result[0].required).toBe(true)
    expect(result[1].required).toBe(false)
  })

  it('marks params with null default as required', () => {
    const result = extractParams(makeObj({ parameters: [{ name: 'param', default: null }] }))
    expect(result[0].required).toBe(true)
  })

  it('skips parsed params without name or description', () => {
    const result = extractParams(makeObj({
      parameters: [{ name: 'x', annotation: { cls: 'ExprName', name: 'int' } }],
      docstring: {
        value: '',
        parsed: [{
          kind: 'parameters',
          value: [
            { name: '', description: 'no name' },
            { name: 'x', description: '' },
            { name: 'x', description: 'valid desc' },
          ],
        }],
      },
    }))

    expect(result[0].description).toBe('valid desc')
  })
})

// ---------------------------------------------------------------------------
// extractExamples
// ---------------------------------------------------------------------------

describe('extractExamples', () => {
  it('returns empty array when no docstring', () => {
    expect(extractExamples(makeObj())).toEqual([])
  })

  it('returns empty array when no Examples section', () => {
    expect(extractExamples(makeObj({ docstring: { value: 'Just a description.' } }))).toEqual([])
  })

  it('extracts code from Examples section', () => {
    const result = extractExamples(makeObj({
      docstring: { value: 'A function.\n\nExamples:\n    result = my_func()\n    print(result)' },
    }))
    expect(result).toEqual(['result = my_func()\nprint(result)'])
  })

  it('extracts code from singular Example section', () => {
    const result = extractExamples(makeObj({
      docstring: { value: 'A function.\n\nExample:\n    result = my_func()' },
    }))
    expect(result).toEqual(['result = my_func()'])
  })

  it('strips doctest >>> prefix', () => {
    const result = extractExamples(makeObj({
      docstring: { value: 'A function.\n\nExamples:\n    >>> x = 1\n    >>> y = 2' },
    }))
    expect(result).toEqual(['x = 1\ny = 2'])
  })

  it('strips doctest ... continuation prefix', () => {
    const result = extractExamples(makeObj({
      docstring: { value: 'A function.\n\nExamples:\n    >>> if True:\n    ...     print("yes")' },
    }))
    expect(result).toEqual(['if True:\n    print("yes")'])
  })

  it('strips variable indentation (4-8 spaces)', () => {
    const result = extractExamples(makeObj({
      docstring: { value: 'A function.\n\nExamples:\n        deeply_indented()' },
    }))
    expect(result).toEqual(['deeply_indented()'])
  })

  it('stops at next Google-style section', () => {
    const result = extractExamples(makeObj({
      docstring: { value: 'A function.\n\nExamples:\n    my_code()\n\nReturns:\n    The result' },
    }))
    expect(result).toEqual(['my_code()'])
  })

  it('returns empty array when example section has only whitespace', () => {
    const result = extractExamples(makeObj({
      docstring: { value: 'A function.\n\nExamples:\n    \n    ' },
    }))
    expect(result).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// isAsync
// ---------------------------------------------------------------------------

describe('isAsync', () => {
  it('returns false when no labels', () => {
    expect(isAsync(makeObj())).toBe(false)
  })

  it('returns false when labels is empty', () => {
    expect(isAsync(makeObj({ labels: [] }))).toBe(false)
  })

  it('returns true when labels contains async', () => {
    expect(isAsync(makeObj({ labels: ['async'] }))).toBe(true)
  })

  it('returns true when labels contains async among others', () => {
    expect(isAsync(makeObj({ labels: ['instance', 'async'] }))).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// buildSignature
// ---------------------------------------------------------------------------

describe('buildSignature', () => {
  it('returns empty parens for no parameters', () => {
    expect(buildSignature(makeObj())).toBe('()')
  })

  it('filters out self and cls', () => {
    expect(buildSignature(makeObj({
      parameters: [
        { name: 'self' },
        { name: 'x', annotation: { cls: 'ExprName', name: 'int' } },
      ],
    }))).toBe('(x: int)')
  })

  it('includes annotation and default', () => {
    expect(buildSignature(makeObj({
      parameters: [
        { name: 'address', annotation: { cls: 'ExprName', name: 'str' } },
        { name: 'port', annotation: { cls: 'ExprName', name: 'int' }, default: '8080' },
      ],
    }))).toBe('(address: str, port: int = 8080)')
  })

  it('includes return type', () => {
    expect(buildSignature(makeObj({
      parameters: [],
      returns: { annotation: { cls: 'ExprName', name: 'None' } },
    }))).toBe('() -> None')
  })

  it('omits annotation when not available', () => {
    expect(buildSignature(makeObj({ parameters: [{ name: 'value' }] }))).toBe('(value)')
  })

  it('handles null default (no default shown)', () => {
    expect(buildSignature(makeObj({ parameters: [{ name: 'x', default: null }] }))).toBe('(x)')
  })

  it('prefixes async for async functions', () => {
    expect(buildSignature(makeObj({
      labels: ['async'],
      parameters: [
        { name: 'self' },
        { name: 'request', annotation: { cls: 'ExprName', name: 'dict' } },
      ],
      returns: { annotation: { cls: 'ExprName', name: 'Any' } },
    }))).toBe('async (request: dict) -> Any')
  })

  it('does not prefix async for sync functions', () => {
    expect(buildSignature(makeObj({
      labels: ['instance'],
      parameters: [{ name: 'x', annotation: { cls: 'ExprName', name: 'int' } }],
    }))).toBe('(x: int)')
  })
})

// ---------------------------------------------------------------------------
// extractAttributeDescriptions
// ---------------------------------------------------------------------------

describe('extractAttributeDescriptions', () => {
  it('returns empty object when no docstring', () => {
    expect(extractAttributeDescriptions(makeObj())).toEqual({})
  })

  it('returns empty object when no Attributes section', () => {
    expect(
      extractAttributeDescriptions(makeObj({ docstring: { value: 'Just a description.' } })),
    ).toEqual({})
  })

  it('extracts single attribute', () => {
    const result = extractAttributeDescriptions(makeObj({
      docstring: { value: 'A class.\n\nAttributes:\n    name: The user name' },
    }))
    expect(result).toEqual({ name: 'The user name' })
  })

  it('extracts multiple attributes', () => {
    const result = extractAttributeDescriptions(makeObj({
      docstring: { value: 'A class.\n\nAttributes:\n    name: The user name\n    age: The user age' },
    }))
    expect(result).toEqual({ name: 'The user name', age: 'The user age' })
  })

  it('handles multi-line attribute descriptions', () => {
    const result = extractAttributeDescriptions(makeObj({
      docstring: { value: 'A class.\n\nAttributes:\n    data: The data payload\n        that spans multiple lines' },
    }))
    expect(result).toEqual({ data: 'The data payload that spans multiple lines' })
  })

  it('stops at next non-indented section', () => {
    const result = extractAttributeDescriptions(makeObj({
      docstring: { value: 'A class.\n\nAttributes:\n    name: The name\n\nMethods are below.' },
    }))
    expect(result).toEqual({ name: 'The name' })
  })

  it('returns empty object for empty Attributes section', () => {
    const result = extractAttributeDescriptions(makeObj({
      docstring: { value: 'A class.\n\nAttributes:\n' },
    }))
    expect(result).toEqual({})
  })
})

// ---------------------------------------------------------------------------
// griffeToFunction
// ---------------------------------------------------------------------------

describe('griffeToFunction', () => {
  it('converts a griffe function object to FunctionDoc', () => {
    const result = griffeToFunction(makeObj({
      name: 'my_func',
      parameters: [
        { name: 'self' },
        { name: 'x', annotation: { cls: 'ExprName', name: 'int' } },
      ],
      returns: { annotation: { cls: 'ExprName', name: 'str' } },
      docstring: {
        value: 'Do something.\n\nArgs:\n    x: the input\n\nExamples:\n    >>> my_func(42)',
        parsed: [
          { kind: 'text', value: 'Do something.' },
          { kind: 'parameters', value: [{ name: 'x', description: 'the input' }] },
        ],
      },
    }))

    expect(result).toEqual({
      name: 'my_func',
      signature: '(x: int) -> str',
      description: 'Do something.',
      params: [{ name: 'x', type: 'int', description: 'the input', required: true }],
      returns: { type: 'str', description: '' },
      examples: ['my_func(42)'],
    })
  })

  it('defaults return type to None when not specified', () => {
    const result = griffeToFunction(makeObj({ name: 'void_func' }))
    expect(result.returns.type).toBe('None')
  })

  it('produces async signature for async functions', () => {
    const result = griffeToFunction(makeObj({
      name: 'trigger_async',
      labels: ['async'],
      parameters: [
        { name: 'self' },
        { name: 'request', annotation: { cls: 'ExprName', name: 'dict' } },
      ],
      returns: { annotation: { cls: 'ExprName', name: 'Any' } },
      docstring: { value: 'Invoke a remote function.' },
    }))

    expect(result.name).toBe('trigger_async')
    expect(result.signature).toBe('async (request: dict) -> Any')
  })

  it('sync function has no async prefix in signature', () => {
    const result = griffeToFunction(makeObj({
      name: 'trigger',
      parameters: [
        { name: 'self' },
        { name: 'request', annotation: { cls: 'ExprName', name: 'dict' } },
      ],
      returns: { annotation: { cls: 'ExprName', name: 'Any' } },
      docstring: { value: 'Invoke a remote function.' },
    }))

    expect(result.signature).toBe('(request: dict) -> Any')
  })
})

// ---------------------------------------------------------------------------
// griffeToType
// ---------------------------------------------------------------------------

describe('griffeToType', () => {
  it('extracts fields from class attribute members', () => {
    const result = griffeToType(makeObj({
      name: 'MyModel',
      kind: 'class',
      docstring: { value: 'A model.' },
      members: {
        name: {
          name: 'name',
          kind: 'attribute',
          annotation: { cls: 'ExprName', name: 'str' },
          docstring: { value: 'The name field.' },
        },
        _private: {
          name: '_private',
          kind: 'attribute',
          annotation: { cls: 'ExprName', name: 'int' },
        },
      },
    }))

    expect(result.name).toBe('MyModel')
    expect(result.description).toBe('A model.')
    expect(result.fields).toHaveLength(1)
    expect(result.fields[0]).toEqual({
      name: 'name',
      type: 'str',
      description: 'The name field.',
      required: true,
    })
  })

  it('uses Attributes: section as fallback for field descriptions', () => {
    const result = griffeToType(makeObj({
      name: 'MyModel',
      kind: 'class',
      docstring: { value: 'A model.\n\nAttributes:\n    host: The server hostname\n    port: The port number' },
      members: {
        host: {
          name: 'host',
          kind: 'attribute',
          annotation: { cls: 'ExprName', name: 'str' },
        },
        port: {
          name: 'port',
          kind: 'attribute',
          annotation: { cls: 'ExprName', name: 'int' },
          value: '8080',
        },
      },
    }))

    expect(result.fields).toEqual([
      { name: 'host', type: 'str', description: 'The server hostname', required: true },
      { name: 'port', type: 'int', description: 'The port number', required: false },
    ])
  })

  it('prefers member docstring over Attributes: fallback', () => {
    const result = griffeToType(makeObj({
      name: 'MyModel',
      kind: 'class',
      docstring: { value: 'A model.\n\nAttributes:\n    name: From attributes section' },
      members: {
        name: {
          name: 'name',
          kind: 'attribute',
          annotation: { cls: 'ExprName', name: 'str' },
          docstring: { value: 'From member docstring.' },
        },
      },
    }))

    expect(result.fields[0].description).toBe('From member docstring.')
  })

  it('falls back to parameters when no members have attributes', () => {
    const result = griffeToType(makeObj({
      name: 'MyModel',
      kind: 'class',
      docstring: { value: 'A model.' },
      members: {},
      parameters: [
        { name: 'self' },
        { name: 'value', annotation: { cls: 'ExprName', name: 'str' } },
      ],
    }))

    expect(result.fields).toEqual([
      { name: 'value', type: 'str', description: '', required: true },
    ])
  })

  it('skips non-attribute members', () => {
    const result = griffeToType(makeObj({
      name: 'MyModel',
      kind: 'class',
      members: {
        my_method: { name: 'my_method', kind: 'function' },
        my_attr: { name: 'my_attr', kind: 'attribute', annotation: { cls: 'ExprName', name: 'int' } },
      },
    }))

    expect(result.fields).toHaveLength(1)
    expect(result.fields[0].name).toBe('my_attr')
  })
})

// ---------------------------------------------------------------------------
// extractTypesFromModule
// ---------------------------------------------------------------------------

describe('extractTypesFromModule', () => {
  it('extracts class types from module members', () => {
    const result = extractTypesFromModule({
      MyType: { name: 'MyType', kind: 'class', docstring: { value: 'A type.' }, members: {} },
      other_func: { name: 'other_func', kind: 'function' },
    }, new Set())

    expect(result).toHaveLength(1)
    expect(result[0].name).toBe('MyType')
  })

  it('skips classes in skipClasses set', () => {
    const result = extractTypesFromModule({
      III: { name: 'III', kind: 'class', members: {} },
      Logger: { name: 'Logger', kind: 'class', members: {} },
      MyType: { name: 'MyType', kind: 'class', docstring: { value: 'A type.' }, members: {} },
    }, new Set(['III', 'Logger']))

    expect(result).toHaveLength(1)
    expect(result[0].name).toBe('MyType')
  })

  it('skips private classes (starting with _)', () => {
    const result = extractTypesFromModule({
      _InternalType: { name: '_InternalType', kind: 'class', members: {} },
      PublicType: { name: 'PublicType', kind: 'class', docstring: { value: 'Public.' }, members: {} },
    }, new Set())

    expect(result).toHaveLength(1)
    expect(result[0].name).toBe('PublicType')
  })
})

// ---------------------------------------------------------------------------
// parseGriffeData (integration)
// ---------------------------------------------------------------------------

describe('parseGriffeData', () => {
  it('returns correct metadata', () => {
    const result = parseGriffeData({ members: {} })

    expect(result.metadata).toEqual({
      language: 'python',
      languageLabel: 'Python',
      title: 'Python SDK',
      description: 'API reference for the iii SDK for Python.',
      installCommand: 'pip install iii-sdk',
      importExample: 'from iii import register_worker, InitOptions',
    })
  })

  it('provides fallback entryPoint when register_worker not found', () => {
    const result = parseGriffeData({ members: {} })

    expect(result.initialization.entryPoint.name).toBe('register_worker')
    expect(result.initialization.entryPoint.description).toBe(
      'Create an III client and auto-start its connection task.',
    )
  })

  it('extracts register_worker from root level', () => {
    const result = parseGriffeData({
      members: {
        register_worker: {
          name: 'register_worker',
          kind: 'function',
          docstring: { value: 'Register a worker.' },
          parameters: [
            { name: 'address', annotation: { cls: 'ExprName', name: 'str' } },
          ],
          returns: { annotation: { cls: 'ExprName', name: 'III' } },
        },
      },
    })

    expect(result.initialization.entryPoint.name).toBe('register_worker')
    expect(result.initialization.entryPoint.description).toBe('Register a worker.')
  })

  it('resolves register_worker from iii.iii submodule when root alias has no docstring', () => {
    const result = parseGriffeData({
      members: {
        register_worker: {
          name: 'register_worker',
          kind: 'function',
        },
        iii: {
          name: 'iii',
          kind: 'module',
          members: {
            register_worker: {
              name: 'register_worker',
              kind: 'function',
              docstring: { value: 'Real implementation docs.' },
              parameters: [
                { name: 'address', annotation: { cls: 'ExprName', name: 'str' } },
              ],
              returns: { annotation: { cls: 'ExprName', name: 'III' } },
            },
          },
        },
      },
    })

    expect(result.initialization.entryPoint.description).toBe('Real implementation docs.')
  })

  it('prefers submodule register_worker over root alias when both have docstrings', () => {
    const result = parseGriffeData({
      members: {
        register_worker: {
          name: 'register_worker',
          kind: 'function',
          docstring: { value: 'Root alias docs.' },
          parameters: [],
        },
        iii: {
          name: 'iii',
          kind: 'module',
          members: {
            register_worker: {
              name: 'register_worker',
              kind: 'function',
              docstring: { value: 'Submodule docs.' },
              parameters: [],
            },
          },
        },
      },
    })

    expect(result.initialization.entryPoint.description).toBe('Submodule docs.')
  })

  it('falls back to root alias when submodule register_worker has no docstring', () => {
    const result = parseGriffeData({
      members: {
        register_worker: {
          name: 'register_worker',
          kind: 'function',
          docstring: { value: 'Root alias docs.' },
          parameters: [],
        },
        iii: {
          name: 'iii',
          kind: 'module',
          members: {
            register_worker: {
              name: 'register_worker',
              kind: 'function',
            },
          },
        },
      },
    })

    expect(result.initialization.entryPoint.description).toBe('Root alias docs.')
  })

  it('extracts methods from III class', () => {
    const result = parseGriffeData({
      members: {
        iii: {
          name: 'iii',
          kind: 'module',
          members: {
            III: {
              name: 'III',
              kind: 'class',
              members: {
                trigger: {
                  name: 'trigger',
                  kind: 'function',
                  docstring: { value: 'Trigger a function.' },
                  parameters: [
                    { name: 'self' },
                    { name: 'name', annotation: { cls: 'ExprName', name: 'str' } },
                  ],
                  returns: { annotation: { cls: 'ExprName', name: 'None' } },
                },
                _internal: {
                  name: '_internal',
                  kind: 'function',
                },
              },
            },
          },
        },
      },
    })

    expect(result.methods).toHaveLength(1)
    expect(result.methods[0].name).toBe('trigger')
  })

  it('extracts both sync and async methods from III class', () => {
    const result = parseGriffeData({
      members: {
        iii: {
          name: 'iii',
          kind: 'module',
          members: {
            III: {
              name: 'III',
              kind: 'class',
              members: {
                trigger: {
                  name: 'trigger',
                  kind: 'function',
                  docstring: { value: 'Invoke a remote function.' },
                  parameters: [
                    { name: 'self' },
                    { name: 'request', annotation: { cls: 'ExprName', name: 'dict' } },
                  ],
                  returns: { annotation: { cls: 'ExprName', name: 'Any' } },
                },
                trigger_async: {
                  name: 'trigger_async',
                  kind: 'function',
                  labels: ['async'],
                  docstring: { value: 'Invoke a remote function (async).' },
                  parameters: [
                    { name: 'self' },
                    { name: 'request', annotation: { cls: 'ExprName', name: 'dict' } },
                  ],
                  returns: { annotation: { cls: 'ExprName', name: 'Any' } },
                },
                _internal: {
                  name: '_internal',
                  kind: 'function',
                  labels: ['async'],
                },
              },
            },
          },
        },
      },
    })

    expect(result.methods).toHaveLength(2)

    const sync = result.methods.find(m => m.name === 'trigger')!
    expect(sync.signature).toBe('(request: dict) -> Any')

    const async_ = result.methods.find(m => m.name === 'trigger_async')!
    expect(async_.signature).toBe('async (request: dict) -> Any')
  })

  it('extracts logger section', () => {
    const result = parseGriffeData({
      members: {
        logger: {
          name: 'logger',
          kind: 'module',
          members: {
            Logger: {
              name: 'Logger',
              kind: 'class',
              docstring: { value: 'Logger utility.' },
              members: {
                info: {
                  name: 'info',
                  kind: 'function',
                  docstring: { value: 'Log info message.' },
                  parameters: [
                    { name: 'self' },
                    { name: 'msg', annotation: { cls: 'ExprName', name: 'str' } },
                  ],
                },
                _debug_internal: {
                  name: '_debug_internal',
                  kind: 'function',
                },
              },
            },
          },
        },
      },
    })

    expect(result.loggerSection).toBeDefined()
    expect(result.loggerSection!.description).toBe('Logger utility.')
    expect(result.loggerSection!.methods).toHaveLength(1)
    expect(result.loggerSection!.methods[0].name).toBe('info')
  })

  it('omits logger section when no Logger class found', () => {
    const result = parseGriffeData({ members: {} })
    expect(result.loggerSection).toBeUndefined()
  })

  it('collects types from modules and root, skipping III and Logger', () => {
    const result = parseGriffeData({
      members: {
        iii: {
          name: 'iii',
          kind: 'module',
          members: {
            III: { name: 'III', kind: 'class', members: {} },
          },
        },
        logger: {
          name: 'logger',
          kind: 'module',
          members: {
            Logger: { name: 'Logger', kind: 'class', members: {} },
          },
        },
        types_module: {
          name: 'types_module',
          kind: 'module',
          members: {
            InitOptions: {
              name: 'InitOptions',
              kind: 'class',
              docstring: { value: 'Init options.' },
              members: {
                timeout: {
                  name: 'timeout',
                  kind: 'attribute',
                  annotation: { cls: 'ExprName', name: 'int' },
                },
              },
            },
          },
        },
        RootType: {
          name: 'RootType',
          kind: 'class',
          docstring: { value: 'A root-level type.' },
          members: {},
        },
      },
    })

    const typeNames = result.types.map(t => t.name)
    expect(typeNames).toContain('InitOptions')
    expect(typeNames).toContain('RootType')
    expect(typeNames).not.toContain('III')
    expect(typeNames).not.toContain('Logger')
  })

  it('handles iii key wrapping the root module', () => {
    const result = parseGriffeData({
      iii: {
        members: {
          register_worker: {
            name: 'register_worker',
            kind: 'function',
            docstring: { value: 'Wrapped root.' },
            parameters: [],
          },
        },
      },
    })

    expect(result.initialization.entryPoint.description).toBe('Wrapped root.')
  })
})
