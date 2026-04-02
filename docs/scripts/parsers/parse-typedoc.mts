import { readFileSync } from 'node:fs'
import type { FunctionDoc, LoggerDoc, ParamDoc, SdkDoc, TypeDoc as TypeDocType, SubpathExport } from '../types.mjs'

interface TypeDocReflection {
  id: number
  name: string
  kind: number
  kindString?: string
  comment?: { summary?: { kind: string; text: string }[]; blockTags?: { tag: string; content: { kind: string; text: string }[] }[] }
  signatures?: TypeDocReflection[]
  parameters?: TypeDocReflection[]
  type?: any
  children?: TypeDocReflection[]
  groups?: { title: string; children: number[] }[]
}

function extractText(summary?: { kind: string; text: string }[]): string {
  if (!summary) return ''
  return summary.map(s => s.text).join('').trim()
}

function extractExamples(comment?: TypeDocReflection['comment']): string[] {
  if (!comment?.blockTags) return []
  return comment.blockTags
    .filter(t => t.tag === '@example')
    .map(t => {
      const text = t.content.map(c => c.text).join('')
      const match = text.match(/```\w*\n([\s\S]*?)```/)
      return match ? match[1].trim() : text.trim()
    })
    .filter(Boolean)
}

function typeToString(type: any): string {
  if (!type) return 'unknown'
  if (type.type === 'intrinsic') return type.name
  if (type.type === 'literal') return JSON.stringify(type.value)
  if (type.type === 'reference') {
    const args = type.typeArguments?.map(typeToString).join(', ')
    return args ? `${type.name}<${args}>` : type.name
  }
  if (type.type === 'union') return type.types.map(typeToString).join(' | ')
  if (type.type === 'reflection' && type.declaration?.signatures) {
    const sig = type.declaration.signatures[0]
    const params = sig.parameters?.map((p: any) => `${p.name}: ${typeToString(p.type)}`).join(', ') ?? ''
    return `(${params}) => ${typeToString(sig.type)}`
  }
  if (type.type === 'array') return `${typeToString(type.elementType)}[]`
  if (type.type === 'mapped' || type.type === 'indexedAccess') return 'unknown'
  return type.name || 'unknown'
}

function extractParams(sig: TypeDocReflection): ParamDoc[] {
  return (sig.parameters ?? []).map(p => ({
    name: p.name,
    type: typeToString(p.type),
    description: extractText(p.comment?.summary),
    required: !(p.type?.type === 'union' && p.type.types?.some((t: any) => t.type === 'intrinsic' && t.name === 'undefined')),
  }))
}

function reflectionToFunction(ref: TypeDocReflection): FunctionDoc | null {
  const sig = ref.signatures?.[0] ?? ref
  const comment = sig.comment ?? ref.comment
  const params = sig.parameters ?? []
  const returnType = typeToString(sig.type ?? ref.type)

  return {
    name: ref.name,
    signature: `(${params.map(p => `${p.name}: ${typeToString(p.type)}`).join(', ')}) => ${returnType}`,
    description: extractText(comment?.summary),
    params: extractParams(sig),
    returns: { type: returnType, description: '' },
    examples: extractExamples(comment),
  }
}

const KIND_ENUM = 8
const KIND_CLASS = 128
const KIND_INTERFACE = 256
const KIND_METHOD = 2048
const KIND_TYPE_ALIAS = 2097152

function extractFieldsFromChildren(children: any[]): ParamDoc[] {
  return children
    .filter((f: any) => f.kind === 1024 || f.kind === 2048) // Property or Method
    .map((f: any) => ({
      name: f.name,
      type: typeToString(f.type ?? f.signatures?.[0]?.type),
      description: extractText(f.comment?.summary ?? f.signatures?.[0]?.comment?.summary),
      required: !(f.flags?.isOptional),
    }))
}

function extractTypesFrom(children: TypeDocReflection[], skipNames: Set<string>): TypeDocType[] {
  const types: TypeDocType[] = []

  for (const child of children) {
    if (skipNames.has(child.name)) continue

    if (child.kind === KIND_INTERFACE) {
      types.push({
        name: child.name,
        description: extractText(child.comment?.summary),
        fields: extractFieldsFromChildren(child.children ?? []),
      })
    } else if (child.kind === KIND_TYPE_ALIAS) {
      const directChildren = child.children ?? child.type?.declaration?.children
      if (directChildren?.length) {
        types.push({
          name: child.name,
          description: extractText(child.comment?.summary),
          fields: extractFieldsFromChildren(directChildren),
        })
      } else if (child.type?.type) {
        types.push({
          name: child.name,
          description: extractText(child.comment?.summary),
          fields: [],
          codeBlock: `type ${child.name} = ${typeToString(child.type)}`,
        })
      }
    } else if (child.kind === KIND_ENUM) {
      const fields: ParamDoc[] = (child.children ?? []).map((f: any) => ({
        name: f.name,
        type: f.type ? typeToString(f.type) : 'string',
        description: extractText(f.comment?.summary),
        required: true,
      }))
      types.push({
        name: child.name,
        description: extractText(child.comment?.summary),
        fields,
      })
    } else if (child.kind === KIND_CLASS) {
      const props = (child.children ?? []).filter((f: any) => f.kind === 1024)
      types.push({
        name: child.name,
        description: extractText(child.comment?.summary),
        fields: extractFieldsFromChildren(props),
      })
    }
  }

  return types
}

function extractLoggerDoc(ref: TypeDocReflection): LoggerDoc | undefined {
  if (!ref || ref.kind !== KIND_CLASS) return undefined
  const description = extractText(ref.comment?.summary)
  const methods: FunctionDoc[] = []
  for (const child of ref.children ?? []) {
    if (child.kind !== KIND_METHOD) continue
    const fn = reflectionToFunction(child)
    if (fn) methods.push(fn)
  }
  if (!description && methods.length === 0) return undefined
  return { description, methods }
}

export function parseTypedoc(jsonPath: string): SdkDoc {
  const raw = JSON.parse(readFileSync(jsonPath, 'utf-8'))

  const modules: TypeDocReflection[] = raw.children ?? []
  const indexModule = modules.find((m: any) => m.name === 'index')
  const streamModule = modules.find((m: any) => m.name === 'stream')
  const stateModule = modules.find((m: any) => m.name === 'state')
  const telemetryModule = modules.find((m: any) => m.name === 'telemetry')

  const allChildren = indexModule?.children ?? []

  const registerWorker = allChildren.find(c => c.name === 'registerWorker')
  const iSdk = allChildren.find(c => c.name === 'ISdk')

  const methods: FunctionDoc[] = []
  if (iSdk?.children) {
    for (const child of iSdk.children) {
      const fn = reflectionToFunction(child)
      if (fn) methods.push(fn)
    }
  }

  const loggerClass = allChildren.find(c => c.name === 'Logger' && c.kind === KIND_CLASS)
  const loggerSection = loggerClass ? extractLoggerDoc(loggerClass) : undefined

  const skipTypes = new Set(['ISdk', 'Logger'])
  const types: TypeDocType[] = [
    ...extractTypesFrom(allChildren, skipTypes),
    ...extractTypesFrom(streamModule?.children ?? [], skipTypes),
    ...extractTypesFrom(stateModule?.children ?? [], skipTypes),
    ...extractTypesFrom(telemetryModule?.children ?? [], skipTypes),
  ]

  const entryFn = registerWorker ? reflectionToFunction(registerWorker) : null

  const subpathExports: SubpathExport[] = [
    { path: 'iii-sdk', description: 'Core SDK exports', exports: allChildren.map(c => c.name) },
  ]
  if (streamModule?.children) {
    subpathExports.push({ path: 'iii-sdk/stream', description: 'Stream types', exports: streamModule.children.map(c => c.name) })
  }
  if (stateModule?.children) {
    subpathExports.push({ path: 'iii-sdk/state', description: 'State management types', exports: stateModule.children.map(c => c.name) })
  }
  if (telemetryModule?.children) {
    subpathExports.push({ path: 'iii-sdk/telemetry', description: 'OpenTelemetry utilities', exports: telemetryModule.children.map(c => c.name) })
  }

  return {
    metadata: {
      language: 'node',
      languageLabel: 'TypeScript',
      title: 'Node.js SDK',
      description: 'API reference for the iii SDK for Node.js / TypeScript.',
      installCommand: 'npm install iii-sdk',
      importExample: "import { registerWorker } from 'iii-sdk'",
    },
    initialization: {
      entryPoint: entryFn ?? { name: 'registerWorker', signature: '(address: string, options?: InitOptions) => ISdk', description: '', params: [], returns: { type: 'ISdk', description: '' }, examples: [] },
    },
    methods,
    types,
    subpathExports,
    loggerSection,
  }
}
