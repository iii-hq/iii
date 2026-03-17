import { describe, it, expect, beforeEach } from 'vitest'
import {
  rustTypeToString,
  extractDocs,
  extractExamples,
  extractArgDescriptions,
  extractReturnDescription,
  parseRustdocData,
} from '../parse-rustdoc.mjs'

// ---------------------------------------------------------------------------
// Helpers for building mock rustdoc JSON items
// ---------------------------------------------------------------------------

let nextId = 1

function resetIds() {
  nextId = 1
}

function id(): number {
  return nextId++
}

function makeItem(
  overrides: Partial<{
    id: number
    name: string | null
    docs: string
    inner: Record<string, any>
    visibility: string
  }>,
) {
  return {
    id: overrides.id ?? id(),
    crate_id: 0,
    name: overrides.name ?? null,
    docs: overrides.docs,
    inner: overrides.inner ?? {},
    visibility: overrides.visibility ?? 'public',
  }
}

// ---------------------------------------------------------------------------
// rustTypeToString
// ---------------------------------------------------------------------------

describe('rustTypeToString', () => {
  it('returns "()" for null/undefined', () => {
    expect(rustTypeToString(null)).toBe('()')
    expect(rustTypeToString(undefined)).toBe('()')
  })

  it('handles primitive types', () => {
    expect(rustTypeToString({ primitive: 'str' })).toBe('str')
    expect(rustTypeToString({ primitive: 'bool' })).toBe('bool')
    expect(rustTypeToString({ primitive: 'u64' })).toBe('u64')
    expect(rustTypeToString({ primitive: 'usize' })).toBe('usize')
  })

  it('handles resolved_path without args', () => {
    expect(rustTypeToString({ resolved_path: { path: 'String', id: 1, args: null } })).toBe(
      'String',
    )
  })

  it('handles resolved_path with generic args', () => {
    const type = {
      resolved_path: {
        path: 'Result',
        id: 1,
        args: {
          angle_bracketed: {
            args: [
              { type: { resolved_path: { path: 'Value', id: 2, args: null } } },
              { type: { resolved_path: { path: 'IIIError', id: 3, args: null } } },
            ],
            constraints: [],
          },
        },
      },
    }
    expect(rustTypeToString(type)).toBe('Result<Value, IIIError>')
  })

  it('handles Option type', () => {
    const type = {
      resolved_path: {
        path: 'Option',
        id: 1,
        args: {
          angle_bracketed: {
            args: [{ type: { primitive: 'usize' } }],
            constraints: [],
          },
        },
      },
    }
    expect(rustTypeToString(type)).toBe('Option<usize>')
  })

  it('handles nested generics (Vec<String>)', () => {
    const type = {
      resolved_path: {
        path: 'Vec',
        id: 1,
        args: {
          angle_bracketed: {
            args: [{ type: { resolved_path: { path: 'String', id: 2, args: null } } }],
            constraints: [],
          },
        },
      },
    }
    expect(rustTypeToString(type)).toBe('Vec<String>')
  })

  it('handles borrowed_ref (immutable)', () => {
    const type = {
      borrowed_ref: {
        lifetime: null,
        is_mutable: false,
        type: { primitive: 'str' },
      },
    }
    expect(rustTypeToString(type)).toBe('&str')
  })

  it('handles borrowed_ref (mutable)', () => {
    const type = {
      borrowed_ref: {
        lifetime: null,
        is_mutable: true,
        type: { primitive: 'str' },
      },
    }
    expect(rustTypeToString(type)).toBe('&mut str')
  })

  it('handles borrowed_ref with lifetime', () => {
    const type = {
      borrowed_ref: {
        lifetime: "'a",
        is_mutable: false,
        type: { primitive: 'str' },
      },
    }
    expect(rustTypeToString(type)).toBe("&'a str")
  })

  it('handles generic type parameter', () => {
    expect(rustTypeToString({ generic: 'T' })).toBe('T')
    expect(rustTypeToString({ generic: 'Self' })).toBe('Self')
  })

  it('handles empty tuple', () => {
    expect(rustTypeToString({ tuple: [] })).toBe('()')
  })

  it('handles tuple types', () => {
    const type = {
      tuple: [{ primitive: 'u32' }, { resolved_path: { path: 'String', id: 1, args: null } }],
    }
    expect(rustTypeToString(type)).toBe('(u32, String)')
  })

  it('handles slice types', () => {
    expect(rustTypeToString({ slice: { primitive: 'u8' } })).toBe('[u8]')
  })

  it('handles array types', () => {
    expect(rustTypeToString({ array: { type: { primitive: 'u8' }, len: 32 } })).toBe('[u8; 32]')
  })

  it('handles raw pointers', () => {
    expect(
      rustTypeToString({ raw_pointer: { is_mutable: false, type: { primitive: 'u8' } } }),
    ).toBe('*const u8')
    expect(
      rustTypeToString({ raw_pointer: { is_mutable: true, type: { primitive: 'u8' } } }),
    ).toBe('*mut u8')
  })

  it('handles impl_trait (impl Into<String>)', () => {
    const type = {
      impl_trait: [
        {
          trait_bound: {
            trait: {
              path: 'Into',
              id: 1,
              args: {
                angle_bracketed: {
                  args: [
                    { type: { resolved_path: { path: 'String', id: 2, args: null } } },
                  ],
                  constraints: [],
                },
              },
            },
            generic_params: [],
            modifier: 'none',
          },
        },
      ],
    }
    expect(rustTypeToString(type)).toBe('impl Into<String>')
  })

  it('simplifies crate-internal paths', () => {
    expect(
      rustTypeToString({
        resolved_path: { path: 'crate::protocol::TriggerRequest', id: 1, args: null },
      }),
    ).toBe('TriggerRequest')
  })

  it('simplifies std paths', () => {
    expect(
      rustTypeToString({
        resolved_path: {
          path: 'std::collections::HashMap',
          id: 1,
          args: {
            angle_bracketed: {
              args: [
                { type: { resolved_path: { path: 'String', id: 2, args: null } } },
                { type: { resolved_path: { path: 'String', id: 3, args: null } } },
              ],
              constraints: [],
            },
          },
        },
      }),
    ).toBe('HashMap<String, String>')
  })

  it('handles serde_json::Value shortening', () => {
    expect(
      rustTypeToString({
        resolved_path: { path: 'serde_json::Value', id: 1, args: null },
      }),
    ).toBe('Value')
  })

  it('returns "unknown" for unrecognized shapes', () => {
    expect(rustTypeToString({ some_unknown_field: true })).toBe('unknown')
  })

  it('handles fn_pointer', () => {
    const type = {
      fn_pointer: {
        sig: {
          inputs: [['x', { primitive: 'u32' }]],
          output: { primitive: 'bool' },
        },
      },
    }
    expect(rustTypeToString(type)).toBe('fn(x: u32) -> bool')
  })

  it('handles string input', () => {
    expect(rustTypeToString('SomeType')).toBe('SomeType')
  })
})

// ---------------------------------------------------------------------------
// extractDocs
// ---------------------------------------------------------------------------

describe('extractDocs', () => {
  it('returns empty string for items with no docs', () => {
    expect(extractDocs(makeItem({}))).toBe('')
  })

  it('extracts description before # section', () => {
    const item = makeItem({
      docs: 'Short description.\n\n# Arguments\n* `x` - the value',
    })
    expect(extractDocs(item)).toBe('Short description.')
  })

  it('extracts description before code blocks', () => {
    const item = makeItem({
      docs: 'Description here.\n\n```rust\nlet x = 1;\n```',
    })
    expect(extractDocs(item)).toBe('Description here.')
  })

  it('returns full text when no sections or code blocks', () => {
    const item = makeItem({ docs: 'Just a plain description.' })
    expect(extractDocs(item)).toBe('Just a plain description.')
  })

  it('handles multi-paragraph descriptions', () => {
    const item = makeItem({
      docs: 'First paragraph.\n\nSecond paragraph.\n\n# Examples\n```\ncode\n```',
    })
    expect(extractDocs(item)).toBe('First paragraph.\n\nSecond paragraph.')
  })
})

// ---------------------------------------------------------------------------
// extractExamples
// ---------------------------------------------------------------------------

describe('extractExamples', () => {
  it('returns empty array when no docs', () => {
    expect(extractExamples(makeItem({}))).toEqual([])
  })

  it('returns empty array when no example section', () => {
    expect(extractExamples(makeItem({ docs: 'Just a description.' }))).toEqual([])
  })

  it('extracts single example', () => {
    const item = makeItem({
      docs: 'Description.\n\n# Example\n\n```rust\nlet x = 1;\n```',
    })
    expect(extractExamples(item)).toEqual(['let x = 1;'])
  })

  it('extracts multiple examples with separate headers', () => {
    const item = makeItem({
      docs: '# Example\n\n```rust\nlet a = 1;\n```\n\n# Example\n\n```rust\nlet b = 2;\n```',
    })
    expect(extractExamples(item)).toEqual(['let a = 1;', 'let b = 2;'])
  })

  it('strips lines starting with # (hidden test lines)', () => {
    const item = makeItem({
      docs: '# Example\n\n```rust\n# use std::io;\nlet x = 1;\n# Ok(())\n```',
    })
    expect(extractExamples(item)).toEqual(['let x = 1;'])
  })
})

// ---------------------------------------------------------------------------
// extractArgDescriptions
// ---------------------------------------------------------------------------

describe('extractArgDescriptions', () => {
  it('returns empty object for undefined docs', () => {
    expect(extractArgDescriptions(undefined)).toEqual({})
  })

  it('returns empty object when no Arguments section', () => {
    expect(extractArgDescriptions('Just a description.')).toEqual({})
  })

  it('extracts single argument', () => {
    const docs = 'Description.\n\n# Arguments\n* `name` - The user name'
    expect(extractArgDescriptions(docs)).toEqual({ name: 'The user name' })
  })

  it('extracts multiple arguments', () => {
    const docs =
      '# Arguments\n* `address` - WebSocket URL.\n* `options` - Configuration options.'
    expect(extractArgDescriptions(docs)).toEqual({
      address: 'WebSocket URL.',
      options: 'Configuration options.',
    })
  })

  it('handles multi-line descriptions', () => {
    const docs = '# Arguments\n* `key` - The stream key\n  in the format "a::b::c"'
    expect(extractArgDescriptions(docs)).toEqual({
      key: 'The stream key in the format "a::b::c"',
    })
  })

  it('stops at next section', () => {
    const docs = '# Arguments\n* `x` - The value\n\n# Returns\nSomething'
    expect(extractArgDescriptions(docs)).toEqual({ x: 'The value' })
  })
})

// ---------------------------------------------------------------------------
// extractReturnDescription
// ---------------------------------------------------------------------------

describe('extractReturnDescription', () => {
  it('returns empty string for undefined docs', () => {
    expect(extractReturnDescription(undefined)).toBe('')
  })

  it('returns empty string when no Returns section', () => {
    expect(extractReturnDescription('Just a description.')).toBe('')
  })

  it('extracts return description', () => {
    const docs = '# Returns\nThe computed value.'
    expect(extractReturnDescription(docs)).toBe('The computed value.')
  })

  it('extracts multi-line return description', () => {
    const docs = '# Returns\n* `UpdateResult` containing\n  the new value.'
    expect(extractReturnDescription(docs)).toBe('UpdateResult containing the new value.')
  })

  it('stops at next section', () => {
    const docs = '# Returns\nThe result.\n\n# Errors\nMay fail.'
    expect(extractReturnDescription(docs)).toBe('The result.')
  })
})

// ---------------------------------------------------------------------------
// parseRustdocData - method extraction
// ---------------------------------------------------------------------------

describe('parseRustdocData', () => {
  beforeEach(() => resetIds())

  function buildMinimalIndex() {
    const rootId = 1000
    const iiiStructId = 100
    const iiiReExportId = 800
    const implId = 200
    const methodId = 300
    const registerWorkerId = 400

    const index: Record<string, any> = {}

    // Root module
    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: {
        module: {
          items: [iiiReExportId, registerWorkerId],
        },
      },
    })

    // III re-export (use item)
    index[iiiReExportId] = makeItem({
      id: iiiReExportId,
      name: null,
      inner: {
        use: { id: iiiStructId, name: 'III', source: 'iii::III', is_glob: false },
      },
    })

    // III struct
    index[iiiStructId] = makeItem({
      id: iiiStructId,
      name: 'III',
      docs: 'WebSocket client for the III Engine.',
      inner: {
        struct: {
          kind: { plain: { fields: [], has_stripped_fields: true } },
          impls: [implId],
        },
      },
    })

    // Inherent impl
    index[implId] = makeItem({
      id: implId,
      name: null,
      inner: {
        impl: {
          trait: null,
          items: [methodId],
          for: { resolved_path: { path: 'III', id: iiiStructId, args: null } },
        },
      },
    })

    // A public method
    index[methodId] = makeItem({
      id: methodId,
      name: 'trigger',
      docs: 'Invoke a remote function.\n\n# Arguments\n* `request` - The trigger request.',
      inner: {
        function: {
          sig: {
            inputs: [
              ['self', { borrowed_ref: { lifetime: null, is_mutable: false, type: { generic: 'Self' } } }],
              ['request', { resolved_path: { path: 'TriggerRequest', id: 50, args: null } }],
            ],
            output: {
              resolved_path: {
                path: 'Result',
                id: 35,
                args: {
                  angle_bracketed: {
                    args: [
                      { type: { resolved_path: { path: 'Value', id: 10, args: null } } },
                      { type: { resolved_path: { path: 'IIIError', id: 20, args: null } } },
                    ],
                    constraints: [],
                  },
                },
              },
            },
            is_c_variadic: false,
          },
          generics: { params: [], where_predicates: [] },
          header: { is_const: false, is_unsafe: false, is_async: true, abi: 'Rust' },
          has_body: true,
        },
      },
    })

    // register_worker function
    index[registerWorkerId] = makeItem({
      id: registerWorkerId,
      name: 'register_worker',
      docs: 'Create and return a connected SDK instance.\n\n# Arguments\n* `address` - WebSocket URL.\n* `options` - Config options.',
      inner: {
        function: {
          sig: {
            inputs: [
              ['address', { borrowed_ref: { lifetime: null, is_mutable: false, type: { primitive: 'str' } } }],
              ['options', { resolved_path: { path: 'InitOptions', id: 60, args: null } }],
            ],
            output: { resolved_path: { path: 'III', id: iiiStructId, args: null } },
            is_c_variadic: false,
          },
          generics: { params: [], where_predicates: [] },
          header: { is_const: false, is_unsafe: false, is_async: false, abi: 'Rust' },
          has_body: true,
        },
      },
    })

    return { root: rootId, format_version: 56, paths: {}, index }
  }

  it('extracts methods from the III struct only', () => {
    const data = buildMinimalIndex()
    const doc = parseRustdocData(data)

    expect(doc.methods).toHaveLength(1)
    expect(doc.methods[0].name).toBe('trigger')
  })

  it('builds proper method signatures', () => {
    const data = buildMinimalIndex()
    const doc = parseRustdocData(data)

    expect(doc.methods[0].signature).toBe(
      'async trigger(request: TriggerRequest) -> Result<Value, IIIError>',
    )
  })

  it('extracts method parameters (skips self)', () => {
    const data = buildMinimalIndex()
    const doc = parseRustdocData(data)

    expect(doc.methods[0].params).toHaveLength(1)
    expect(doc.methods[0].params[0]).toEqual({
      name: 'request',
      type: 'TriggerRequest',
      description: 'The trigger request.',
      required: true,
    })
  })

  it('extracts return types', () => {
    const data = buildMinimalIndex()
    const doc = parseRustdocData(data)

    expect(doc.methods[0].returns.type).toBe('Result<Value, IIIError>')
  })

  it('extracts method descriptions', () => {
    const data = buildMinimalIndex()
    const doc = parseRustdocData(data)

    expect(doc.methods[0].description).toBe('Invoke a remote function.')
  })

  it('extracts register_worker as entry point', () => {
    const data = buildMinimalIndex()
    const doc = parseRustdocData(data)

    expect(doc.initialization.entryPoint.name).toBe('register_worker')
    expect(doc.initialization.entryPoint.signature).toBe(
      'register_worker(address: &str, options: InitOptions) -> III',
    )
    expect(doc.initialization.entryPoint.params).toHaveLength(2)
    expect(doc.initialization.entryPoint.params[0].name).toBe('address')
    expect(doc.initialization.entryPoint.params[0].description).toBe('WebSocket URL.')
    expect(doc.initialization.entryPoint.params[1].name).toBe('options')
  })

  it('uses fallback entry point when register_worker is missing', () => {
    const data = buildMinimalIndex()
    delete data.index[400]
    data.index[data.root].inner.module.items = [800] // just the III re-export
    const doc = parseRustdocData(data)

    expect(doc.initialization.entryPoint.name).toBe('register_worker')
    expect(doc.initialization.entryPoint.signature).toContain('register_worker')
  })

  it('excludes methods in METHOD_EXCLUDE set', () => {
    const data = buildMinimalIndex()

    // Add a 'new' method (should be excluded)
    const newMethodId = 301
    data.index[newMethodId] = makeItem({
      id: newMethodId,
      name: 'new',
      docs: 'Create a new III.',
      inner: {
        function: {
          sig: {
            inputs: [['address', { borrowed_ref: { lifetime: null, is_mutable: false, type: { primitive: 'str' } } }]],
            output: { resolved_path: { path: 'III', id: 100, args: null } },
            is_c_variadic: false,
          },
          generics: { params: [], where_predicates: [] },
          header: { is_const: false, is_unsafe: false, is_async: false, abi: 'Rust' },
          has_body: true,
        },
      },
    })
    data.index[200].inner.impl.items.push(newMethodId)

    const doc = parseRustdocData(data)
    expect(doc.methods.map((m: any) => m.name)).not.toContain('new')
    expect(doc.methods.map((m: any) => m.name)).toContain('trigger')
  })

  it('excludes non-public methods', () => {
    const data = buildMinimalIndex()

    const privateMethodId = 302
    data.index[privateMethodId] = makeItem({
      id: privateMethodId,
      name: 'internal_method',
      visibility: 'default',
      inner: {
        function: {
          sig: { inputs: [], output: null, is_c_variadic: false },
          generics: { params: [], where_predicates: [] },
          header: { is_const: false, is_unsafe: false, is_async: false, abi: 'Rust' },
          has_body: true,
        },
      },
    })
    data.index[200].inner.impl.items.push(privateMethodId)

    const doc = parseRustdocData(data)
    expect(doc.methods.map((m: any) => m.name)).not.toContain('internal_method')
  })

  it('skips trait impls', () => {
    const data = buildMinimalIndex()

    const traitImplId = 201
    const traitMethodId = 303
    data.index[traitImplId] = makeItem({
      id: traitImplId,
      name: null,
      inner: {
        impl: {
          trait: { path: 'Clone', id: 99 },
          items: [traitMethodId],
          for: { resolved_path: { path: 'III', id: 100, args: null } },
        },
      },
    })
    data.index[traitMethodId] = makeItem({
      id: traitMethodId,
      name: 'clone',
      inner: {
        function: {
          sig: {
            inputs: [['self', { borrowed_ref: { lifetime: null, is_mutable: false, type: { generic: 'Self' } } }]],
            output: { generic: 'Self' },
            is_c_variadic: false,
          },
          generics: { params: [], where_predicates: [] },
          header: { is_const: false, is_unsafe: false, is_async: false, abi: 'Rust' },
          has_body: true,
        },
      },
    })
    data.index[100].inner.struct.impls.push(traitImplId)

    const doc = parseRustdocData(data)
    expect(doc.methods.map((m: any) => m.name)).not.toContain('clone')
  })
})

// ---------------------------------------------------------------------------
// parseRustdocData - struct field extraction
// ---------------------------------------------------------------------------

describe('parseRustdocData - struct fields', () => {
  beforeEach(() => resetIds())

  it('extracts public struct fields', () => {
    const rootId = 1000
    const structId = 100
    const field1Id = 101
    const field2Id = 102

    const index: Record<string, any> = {}

    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: { module: { items: [900] } },
    })

    index[900] = makeItem({
      id: 900,
      name: null,
      inner: { use: { id: structId, name: 'TriggerRequest', source: 'protocol::TriggerRequest', is_glob: false } },
    })

    index[structId] = makeItem({
      id: structId,
      name: 'TriggerRequest',
      docs: 'Request for triggering.',
      inner: {
        struct: {
          kind: { plain: { fields: [field1Id, field2Id], has_stripped_fields: false } },
          impls: [],
        },
      },
    })

    index[field1Id] = makeItem({
      id: field1Id,
      name: 'function_id',
      docs: 'ID of the function to invoke.',
      inner: { struct_field: { resolved_path: { path: 'String', id: 1, args: null } } },
    })

    index[field2Id] = makeItem({
      id: field2Id,
      name: 'action',
      inner: {
        struct_field: {
          resolved_path: {
            path: 'Option',
            id: 2,
            args: {
              angle_bracketed: {
                args: [{ type: { resolved_path: { path: 'TriggerAction', id: 3, args: null } } }],
                constraints: [],
              },
            },
          },
        },
      },
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    const type = doc.types.find((t) => t.name === 'TriggerRequest')
    expect(type).toBeDefined()
    expect(type!.fields).toHaveLength(2)
    expect(type!.fields[0]).toEqual({
      name: 'function_id',
      type: 'String',
      description: 'ID of the function to invoke.',
      required: true,
    })
    expect(type!.fields[1]).toEqual({
      name: 'action',
      type: 'Option<TriggerAction>',
      description: '',
      required: false,
    })
  })

  it('adds methods-as-fields for structs with no public fields', () => {
    const rootId = 1000
    const structId = 100
    const implId = 200
    const methodId = 300

    const index: Record<string, any> = {}

    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: { module: { items: [900] } },
    })

    index[900] = makeItem({
      id: 900,
      name: null,
      inner: { use: { id: structId, name: 'Streams', source: 'stream::Streams', is_glob: false } },
    })

    index[structId] = makeItem({
      id: structId,
      name: 'Streams',
      docs: 'Stream operations.',
      inner: {
        struct: {
          kind: { plain: { fields: [], has_stripped_fields: true } },
          impls: [implId],
        },
      },
    })

    index[implId] = makeItem({
      id: implId,
      name: null,
      inner: {
        impl: {
          trait: null,
          items: [methodId],
          for: { resolved_path: { path: 'Streams', id: structId, args: null } },
        },
      },
    })

    index[methodId] = makeItem({
      id: methodId,
      name: 'update',
      docs: 'Atomic update.',
      inner: {
        function: {
          sig: {
            inputs: [
              ['self', { borrowed_ref: { lifetime: null, is_mutable: false, type: { generic: 'Self' } } }],
              ['key', { borrowed_ref: { lifetime: null, is_mutable: false, type: { primitive: 'str' } } }],
            ],
            output: {
              resolved_path: {
                path: 'Result',
                id: 1,
                args: {
                  angle_bracketed: {
                    args: [
                      { type: { resolved_path: { path: 'UpdateResult', id: 2, args: null } } },
                      { type: { resolved_path: { path: 'IIIError', id: 3, args: null } } },
                    ],
                    constraints: [],
                  },
                },
              },
            },
            is_c_variadic: false,
          },
          generics: { params: [], where_predicates: [] },
          header: { is_const: false, is_unsafe: false, is_async: true, abi: 'Rust' },
          has_body: true,
        },
      },
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    const type = doc.types.find((t) => t.name === 'Streams')
    expect(type).toBeDefined()
    expect(type!.fields).toHaveLength(1)
    expect(type!.fields[0].name).toBe('update')
    expect(type!.fields[0].type).toBe('async fn(key: &str) -> Result<UpdateResult, IIIError>')
    expect(type!.fields[0].description).toBe('Atomic update.')
  })
})

// ---------------------------------------------------------------------------
// parseRustdocData - enum extraction
// ---------------------------------------------------------------------------

describe('parseRustdocData - enum extraction', () => {
  beforeEach(() => resetIds())

  it('extracts enum variants', () => {
    const rootId = 1000
    const enumId = 100
    const variant1Id = 101
    const variant2Id = 102
    const variantFieldId = 103

    const index: Record<string, any> = {}

    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: { module: { items: [900] } },
    })

    index[900] = makeItem({
      id: 900,
      name: null,
      inner: { use: { id: enumId, name: 'TriggerAction', source: 'protocol::TriggerAction', is_glob: false } },
    })

    index[enumId] = makeItem({
      id: enumId,
      name: 'TriggerAction',
      docs: 'Routing action.',
      inner: {
        enum: {
          generics: { params: [], where_predicates: [] },
          has_stripped_variants: false,
          variants: [variant1Id, variant2Id],
          impls: [],
        },
      },
    })

    index[variant1Id] = makeItem({
      id: variant1Id,
      name: 'Enqueue',
      docs: 'Queue routing.',
      inner: {
        variant: {
          kind: {
            struct: {
              fields: [variantFieldId],
              has_stripped_fields: false,
            },
          },
          discriminant: null,
        },
      },
    })

    index[variantFieldId] = makeItem({
      id: variantFieldId,
      name: 'queue',
      inner: { struct_field: { resolved_path: { path: 'String', id: 1, args: null } } },
    })

    index[variant2Id] = makeItem({
      id: variant2Id,
      name: 'Void',
      docs: 'Fire-and-forget.',
      inner: {
        variant: {
          kind: 'plain',
          discriminant: null,
        },
      },
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    const type = doc.types.find((t) => t.name === 'TriggerAction')
    expect(type).toBeDefined()
    expect(type!.fields).toHaveLength(2)
    expect(type!.fields[0]).toEqual({
      name: 'Enqueue',
      type: '{ queue: String }',
      description: 'Queue routing.',
      required: true,
    })
    expect(type!.fields[1]).toEqual({
      name: 'Void',
      type: 'unit',
      description: 'Fire-and-forget.',
      required: true,
    })
  })

  it('extracts simple unit enum variants', () => {
    const rootId = 1000
    const enumId = 100

    const index: Record<string, any> = {}

    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: { module: { items: [900] } },
    })

    index[900] = makeItem({
      id: 900,
      name: null,
      inner: { use: { id: enumId, name: 'IIIConnectionState', source: 'iii::IIIConnectionState', is_glob: false } },
    })

    index[enumId] = makeItem({
      id: enumId,
      name: 'IIIConnectionState',
      docs: 'Connection state.',
      inner: {
        enum: {
          variants: [111, 112],
          impls: [],
          has_stripped_variants: false,
        },
      },
    })

    index[111] = makeItem({ id: 111, name: 'Connected', inner: { variant: { kind: 'plain', discriminant: null } } })
    index[112] = makeItem({ id: 112, name: 'Disconnected', docs: 'Not connected.', inner: { variant: { kind: 'plain', discriminant: null } } })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    const type = doc.types.find((t) => t.name === 'IIIConnectionState')
    expect(type).toBeDefined()
    expect(type!.fields).toHaveLength(2)
    expect(type!.fields[0].name).toBe('Connected')
    expect(type!.fields[0].type).toBe('unit')
    expect(type!.fields[1].name).toBe('Disconnected')
    expect(type!.fields[1].description).toBe('Not connected.')
  })
})

// ---------------------------------------------------------------------------
// parseRustdocData - filtering
// ---------------------------------------------------------------------------

describe('parseRustdocData - filtering', () => {
  beforeEach(() => resetIds())

  it('only includes types in EXPORTED_TYPES set', () => {
    const rootId = 1000
    const index: Record<string, any> = {}

    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: {
        module: {
          items: [901, 902],
        },
      },
    })

    // TriggerRequest is in EXPORTED_TYPES
    index[901] = makeItem({
      id: 901,
      name: null,
      inner: { use: { id: 50, name: 'TriggerRequest', source: 'protocol::TriggerRequest', is_glob: false } },
    })

    index[50] = makeItem({
      id: 50,
      name: 'TriggerRequest',
      docs: 'A request.',
      inner: {
        struct: {
          kind: { plain: { fields: [], has_stripped_fields: false } },
          impls: [],
        },
      },
    })

    // SharedConnection is NOT in EXPORTED_TYPES
    index[902] = makeItem({
      id: 902,
      name: null,
      inner: { use: { id: 51, name: 'SharedConnection', source: 'connection::SharedConnection', is_glob: false } },
    })

    index[51] = makeItem({
      id: 51,
      name: 'SharedConnection',
      docs: 'Internal connection.',
      inner: {
        struct: {
          kind: { plain: { fields: [], has_stripped_fields: true } },
          impls: [],
        },
      },
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    expect(doc.types.map((t) => t.name)).toContain('TriggerRequest')
    expect(doc.types.map((t) => t.name)).not.toContain('SharedConnection')
  })

  it('does not include internal functions as methods', () => {
    const rootId = 1000
    const index: Record<string, any> = {}

    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: {
        module: {
          items: [901, 903],
        },
      },
    })

    // Re-export of an internal function (bytes_to_hex)
    index[901] = makeItem({
      id: 901,
      name: null,
      inner: { use: { id: 52, name: 'bytes_to_hex', source: 'telemetry::bytes_to_hex', is_glob: false } },
    })

    index[52] = makeItem({
      id: 52,
      name: 'bytes_to_hex',
      inner: {
        function: {
          sig: { inputs: [['data', { slice: { primitive: 'u8' } }]], output: { resolved_path: { path: 'String', id: 1, args: null } } },
          generics: { params: [], where_predicates: [] },
          header: { is_const: false, is_unsafe: false, is_async: false, abi: 'Rust' },
          has_body: true,
        },
      },
    })

    // Direct function in module (register_worker)
    index[903] = makeItem({
      id: 903,
      name: 'register_worker',
      docs: 'Entry point.',
      inner: {
        function: {
          sig: {
            inputs: [['address', { borrowed_ref: { lifetime: null, is_mutable: false, type: { primitive: 'str' } } }]],
            output: { resolved_path: { path: 'III', id: 100, args: null } },
          },
          generics: { params: [], where_predicates: [] },
          header: { is_const: false, is_unsafe: false, is_async: false, abi: 'Rust' },
          has_body: true,
        },
      },
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    // bytes_to_hex should NOT appear in methods (only III struct methods go there)
    expect(doc.methods.map((m) => m.name)).not.toContain('bytes_to_hex')
    // register_worker is extracted as entry point, not a method
    expect(doc.methods.map((m) => m.name)).not.toContain('register_worker')
  })
})

// ---------------------------------------------------------------------------
// parseRustdocData - direct root items
// ---------------------------------------------------------------------------

describe('parseRustdocData - direct root items', () => {
  beforeEach(() => resetIds())

  it('includes InitOptions defined directly in root module', () => {
    const rootId = 1000
    const structId = 500

    const index: Record<string, any> = {}

    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: {
        module: {
          items: [structId],
        },
      },
    })

    const fieldId = 501
    index[structId] = makeItem({
      id: structId,
      name: 'InitOptions',
      docs: 'Configuration options.',
      inner: {
        struct: {
          kind: { plain: { fields: [fieldId], has_stripped_fields: false } },
          impls: [],
        },
      },
    })

    index[fieldId] = makeItem({
      id: fieldId,
      name: 'metadata',
      docs: 'Custom worker metadata.',
      inner: {
        struct_field: {
          resolved_path: {
            path: 'Option',
            id: 1,
            args: {
              angle_bracketed: {
                args: [{ type: { resolved_path: { path: 'WorkerMetadata', id: 2, args: null } } }],
                constraints: [],
              },
            },
          },
        },
      },
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    const type = doc.types.find((t) => t.name === 'InitOptions')
    expect(type).toBeDefined()
    expect(type!.description).toBe('Configuration options.')
    expect(type!.fields).toHaveLength(1)
    expect(type!.fields[0].name).toBe('metadata')
    expect(type!.fields[0].type).toBe('Option<WorkerMetadata>')
    expect(type!.fields[0].required).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// parseRustdocData - metadata structure
// ---------------------------------------------------------------------------

describe('parseRustdocData - metadata', () => {
  beforeEach(() => resetIds())

  it('always includes correct metadata', () => {
    const rootId = 1000
    const index: Record<string, any> = {}
    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: { module: { items: [] } },
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    expect(doc.metadata).toEqual({
      language: 'rust',
      languageLabel: 'Rust',
      title: 'Rust SDK',
      description: 'API reference for the iii SDK for Rust.',
      installCommand: 'cargo add iii-sdk',
      importExample: 'use iii_sdk::{register_worker, InitOptions};',
    })
  })

  it('extracts logger section from Logger struct', () => {
    const rootId = 1000
    const index: Record<string, any> = {}

    const infoMethodId = id()
    const loggerImplId = id()
    const loggerStructId = id()
    const loggerReExportId = id()

    index[infoMethodId] = makeItem({
      id: infoMethodId,
      name: 'info',
      docs: 'Log an info-level message.\n\n# Arguments\n\n* `message` - Human-readable log message.\n* `data` - Structured context.',
      inner: {
        function: {
          sig: {
            inputs: [
              ['self', { borrowed_ref: { type: { generic: 'Self' } } }],
              ['message', { primitive: 'str' }],
              ['data', { resolved_path: { path: 'Option', args: { angle_bracketed: { args: [{ type: { resolved_path: { path: 'Value', id: 99, args: null } } }] } } } }],
            ],
            output: null,
          },
          header: { is_async: false },
        },
      },
    })

    index[loggerImplId] = makeItem({
      id: loggerImplId,
      name: null,
      inner: { impl: { trait: null, items: [infoMethodId] } },
    })

    index[loggerStructId] = makeItem({
      id: loggerStructId,
      name: 'Logger',
      docs: 'Structured logger that emits logs as OpenTelemetry LogRecords.',
      inner: { struct: { kind: { plain: { fields: [] } }, impls: [loggerImplId] } },
    })

    index[loggerReExportId] = makeItem({
      id: loggerReExportId,
      name: 'Logger',
      inner: { use: { id: loggerStructId, name: 'Logger' } },
    })

    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: { module: { items: [loggerReExportId] } },
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    expect(doc.loggerSection).toBeDefined()
    expect(doc.loggerSection!.description).toContain('OpenTelemetry')
    expect(doc.loggerSection!.methods.length).toBeGreaterThan(0)
    expect(doc.loggerSection!.methods[0].name).toBe('info')
  })

  it('returns undefined loggerSection when no Logger struct', () => {
    const rootId = 1000
    const index: Record<string, any> = {}
    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: { module: { items: [] } },
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    expect(doc.loggerSection).toBeUndefined()
  })
})

// ---------------------------------------------------------------------------
// parseRustdocData - name collision avoidance
// ---------------------------------------------------------------------------

describe('parseRustdocData - name collisions', () => {
  beforeEach(() => resetIds())

  it('does not produce duplicate "new" methods from different structs', () => {
    const rootId = 1000
    const iiiStructId = 100
    const streamsStructId = 110
    const implId1 = 200
    const implId2 = 210
    const newMethod1 = 300
    const newMethod2 = 310
    const triggerMethod = 320

    const index: Record<string, any> = {}

    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: {
        module: {
          items: [901, 902],
        },
      },
    })

    index[901] = makeItem({
      id: 901,
      name: null,
      inner: { use: { id: iiiStructId, name: 'III', source: 'iii::III', is_glob: false } },
    })

    index[902] = makeItem({
      id: 902,
      name: null,
      inner: { use: { id: streamsStructId, name: 'Streams', source: 'stream::Streams', is_glob: false } },
    })

    // III struct with 'new' and 'trigger'
    index[iiiStructId] = makeItem({
      id: iiiStructId,
      name: 'III',
      inner: {
        struct: {
          kind: { plain: { fields: [], has_stripped_fields: true } },
          impls: [implId1],
        },
      },
    })

    index[implId1] = makeItem({
      id: implId1,
      inner: { impl: { trait: null, items: [newMethod1, triggerMethod] } },
    })

    const fnSig = (inputs: any[], output: any, isAsync = false) => ({
      function: {
        sig: { inputs, output, is_c_variadic: false },
        generics: { params: [], where_predicates: [] },
        header: { is_const: false, is_unsafe: false, is_async: isAsync, abi: 'Rust' },
        has_body: true,
      },
    })

    index[newMethod1] = makeItem({
      id: newMethod1,
      name: 'new',
      inner: fnSig(
        [['addr', { borrowed_ref: { lifetime: null, is_mutable: false, type: { primitive: 'str' } } }]],
        { resolved_path: { path: 'III', id: iiiStructId, args: null } },
      ),
    })

    index[triggerMethod] = makeItem({
      id: triggerMethod,
      name: 'trigger',
      inner: fnSig(
        [
          ['self', { borrowed_ref: { lifetime: null, is_mutable: false, type: { generic: 'Self' } } }],
          ['req', { resolved_path: { path: 'TriggerRequest', id: 50, args: null } }],
        ],
        { resolved_path: { path: 'Result', id: 1, args: { angle_bracketed: { args: [{ type: { resolved_path: { path: 'Value', id: 2, args: null } } }, { type: { resolved_path: { path: 'IIIError', id: 3, args: null } } }], constraints: [] } } } },
        true,
      ),
    })

    // Streams struct with 'new'
    index[streamsStructId] = makeItem({
      id: streamsStructId,
      name: 'Streams',
      inner: {
        struct: {
          kind: { plain: { fields: [], has_stripped_fields: true } },
          impls: [implId2],
        },
      },
    })

    index[implId2] = makeItem({
      id: implId2,
      inner: { impl: { trait: null, items: [newMethod2] } },
    })

    index[newMethod2] = makeItem({
      id: newMethod2,
      name: 'new',
      inner: fnSig(
        [['iii', { borrowed_ref: { lifetime: null, is_mutable: false, type: { resolved_path: { path: 'III', id: iiiStructId, args: null } } } }]],
        { resolved_path: { path: 'Streams', id: streamsStructId, args: null } },
      ),
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    // Methods should only contain III methods, excluding 'new'
    expect(doc.methods.map((m: any) => m.name)).toEqual(['trigger'])
    expect(doc.methods.filter((m: any) => m.name === 'new')).toHaveLength(0)

    // Streams type should not include 'new' either (excluded by extractMethodsAsFields)
    const streamsType = doc.types.find((t) => t.name === 'Streams')
    expect(streamsType).toBeDefined()
    expect(streamsType!.fields.map((f) => f.name)).not.toContain('new')
  })
})

// ---------------------------------------------------------------------------
// parseRustdocData - empty / fallback
// ---------------------------------------------------------------------------

describe('parseRustdocData - edge cases', () => {
  beforeEach(() => resetIds())

  it('returns empty doc when root item is missing', () => {
    const data = { root: 9999, format_version: 56, paths: {}, index: {} }
    const doc = parseRustdocData(data)

    expect(doc.methods).toEqual([])
    expect(doc.types).toEqual([])
    expect(doc.metadata.language).toBe('rust')
  })

  it('handles empty module items', () => {
    const rootId = 1000
    const index: Record<string, any> = {}
    index[rootId] = makeItem({
      id: rootId,
      name: 'iii_sdk',
      inner: { module: { items: [] } },
    })

    const data = { root: rootId, format_version: 56, paths: {}, index }
    const doc = parseRustdocData(data)

    expect(doc.methods).toEqual([])
    expect(doc.types).toEqual([])
  })
})
