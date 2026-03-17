import { readFileSync } from 'node:fs'
import type { FunctionDoc, LoggerDoc, ParamDoc, SdkDoc, TypeDoc } from '../types.mjs'

interface GriffeDocstring {
  value?: string
  parsed?: GriffeParsedSection[]
}

interface GriffeParsedSection {
  kind: string
  value?: string | GriffeParam[]
}

interface GriffeParam {
  name: string
  description?: string
  annotation?: { source?: string }
}

interface GriffeObject {
  name: string
  kind: string
  docstring?: GriffeDocstring
  members?: Record<string, GriffeObject>
  parameters?: { name: string; annotation?: { source?: string }; default?: string | null }[]
  returns?: { annotation?: { source?: string } }
  labels?: string[]
  annotation?: { source?: string }
  value?: string | null
}

export function annotationToString(ann: any): string {
  if (!ann) return ''
  if (typeof ann === 'string') return ann

  switch (ann.cls) {
    case 'ExprName':
      return ann.name ?? ''
    case 'ExprBinOp': {
      const left = annotationToString(ann.left)
      const right = annotationToString(ann.right)
      const op = ann.operator ?? '|'
      if (!left && !right) return ''
      if (!left) return right
      if (!right) return left
      return `${left} ${op} ${right}`
    }
    case 'ExprSubscript': {
      const base = annotationToString(ann.left)
      const slice = annotationToString(ann.slice)
      if (!base) return ''
      return slice ? `${base}[${slice}]` : base
    }
    case 'ExprTuple':
      return (ann.elements ?? []).map(annotationToString).filter(Boolean).join(', ')
    case 'ExprAttribute':
      return ann.member ?? ann.name ?? ''
    default:
      return ann.source ?? ''
  }
}

export function extractDocstring(obj: GriffeObject): string {
  if (!obj.docstring?.parsed) {
    // Strip all Google-style sections from raw docstring
    return obj.docstring?.value
      ?.split(/\n\n(?:Args|Attributes|Returns|Raises|Examples?|Note|Yields|See Also):/)[0]
      ?.trim() ?? ''
  }
  const textParts = obj.docstring.parsed.filter(p => p.kind === 'text')
  return textParts.map(p => typeof p.value === 'string' ? p.value : '').join('\n').trim()
}

export function extractParams(obj: GriffeObject): ParamDoc[] {
  const docParams: Record<string, string> = {}

  if (obj.docstring?.parsed) {
    for (const section of obj.docstring.parsed) {
      if (section.kind === 'parameters' && Array.isArray(section.value)) {
        for (const param of section.value as GriffeParam[]) {
          if (param.name && param.description) {
            docParams[param.name] = param.description
          }
        }
      }
    }
  }

  // Fallback: parse Args section from raw docstring
  if (Object.keys(docParams).length === 0 && obj.docstring?.value) {
    const argsMatch = obj.docstring.value.match(/Args:\n([\s\S]*?)(?:\n\n|\nReturns:|\nRaises:|\nExamples?:|\n\S|$)/)
    if (argsMatch) {
      const argLines = argsMatch[1].split('\n')
      let currentParam = ''
      let currentDesc = ''
      for (const line of argLines) {
        const paramMatch = line.match(/^\s{4,8}(\w+):\s*(.*)/)
        if (paramMatch) {
          if (currentParam) docParams[currentParam] = currentDesc.trim()
          currentParam = paramMatch[1]
          currentDesc = paramMatch[2]
        } else if (currentParam && line.match(/^\s{8,}/)) {
          currentDesc += ' ' + line.trim()
        }
      }
      if (currentParam) docParams[currentParam] = currentDesc.trim()
    }
  }

  return (obj.parameters ?? [])
    .filter(p => p.name !== 'self' && p.name !== 'cls')
    .map(p => ({
      name: p.name,
      type: annotationToString(p.annotation) || 'Any',
      description: docParams[p.name] ?? '',
      required: p.default === undefined || p.default === null,
    }))
}

export function extractExamples(obj: GriffeObject): string[] {
  if (!obj.docstring?.value) return []
  const exampleMatch = obj.docstring.value.match(/Examples?:\n([\s\S]*?)(?:\n\n(?:[A-Z]\w*:)|\n\n\S|$)/)
  if (!exampleMatch) return []
  const code = exampleMatch[1]
    .split('\n')
    .map(l => l.replace(/^\s{4,8}/, ''))   // strip leading indentation
    .map(l => l.replace(/^>>> /, ''))        // strip doctest >>> prefix
    .map(l => l.replace(/^\.\.\. /, ''))     // strip doctest ... prefix
    .filter(l => l.trim())
    .join('\n')
  return code ? [code] : []
}

export function isAsync(obj: GriffeObject): boolean {
  return (obj.labels ?? []).includes('async')
}

export function buildSignature(obj: GriffeObject): string {
  const asyncPrefix = isAsync(obj) ? 'async ' : ''
  const params = (obj.parameters ?? [])
    .filter(p => p.name !== 'self' && p.name !== 'cls')
    .map(p => {
      const annStr = annotationToString(p.annotation)
      const ann = annStr ? `: ${annStr}` : ''
      const def = p.default !== undefined && p.default !== null ? ` = ${p.default}` : ''
      return `${p.name}${ann}${def}`
    })
    .join(', ')
  const retStr = annotationToString(obj.returns?.annotation)
  const ret = retStr ? ` -> ${retStr}` : ''
  return `${asyncPrefix}(${params})${ret}`
}

export function griffeToFunction(obj: GriffeObject): FunctionDoc {
  return {
    name: obj.name,
    signature: buildSignature(obj),
    description: extractDocstring(obj),
    params: extractParams(obj),
    returns: {
      type: annotationToString(obj.returns?.annotation) || 'None',
      description: '',
    },
    examples: extractExamples(obj),
  }
}

export function extractAttributeDescriptions(obj: GriffeObject): Record<string, string> {
  const docstring = obj.docstring?.value ?? ''
  const attrMatch = docstring.match(/Attributes:\n([\s\S]*?)(?:\n\n\S|\n\n$|$)/)
  if (!attrMatch) return {}
  const result: Record<string, string> = {}
  let currentAttr = ''
  let currentDesc = ''
  for (const line of attrMatch[1].split('\n')) {
    const attrLine = line.match(/^\s{4,8}(\w+):\s*(.*)/)
    if (attrLine) {
      if (currentAttr) result[currentAttr] = currentDesc.trim()
      currentAttr = attrLine[1]
      currentDesc = attrLine[2]
    } else if (currentAttr && line.match(/^\s{8,}/)) {
      currentDesc += ' ' + line.trim()
    }
  }
  if (currentAttr) result[currentAttr] = currentDesc.trim()
  return result
}

export function griffeToType(obj: GriffeObject): TypeDoc {
  const fields: ParamDoc[] = []
  const attrDescs = extractAttributeDescriptions(obj)

  if (obj.members) {
    for (const [name, member] of Object.entries(obj.members)) {
      if (name.startsWith('_')) continue
      if (member.kind === 'attribute') {
        fields.push({
          name: member.name,
          type: annotationToString(member.annotation) || 'Any',
          description: extractDocstring(member) || attrDescs[member.name] || '',
          required: member.value === undefined || member.value === null,
        })
      }
    }
  }

  if (fields.length === 0 && obj.parameters) {
    fields.push(...extractParams(obj))
  }

  return {
    name: obj.name,
    description: extractDocstring(obj),
    fields,
  }
}

export function extractTypesFromModule(members: Record<string, GriffeObject>, skipClasses: Set<string>): TypeDoc[] {
  const types: TypeDoc[] = []

  for (const [name, member] of Object.entries(members)) {
    if (name.startsWith('_')) continue
    if (member.kind === 'class' && !skipClasses.has(name)) {
      types.push(griffeToType(member))
    }
  }

  return types
}

export function parseGriffeData(raw: Record<string, any>): SdkDoc {
  const rootModule = raw['iii'] ?? raw
  const rootMembers: Record<string, GriffeObject> = rootModule.members ?? {}

  const iiiSubmodule = rootMembers['iii']
  const iiiSubMembers: Record<string, GriffeObject> = iiiSubmodule?.members ?? {}
  const iiiClass: GriffeObject | undefined = iiiSubMembers['III']

  // register_worker at root level is an alias (re-export) with no metadata.
  // Resolve the real definition from the iii.iii submodule.
  const registerWorkerAlias = rootMembers['register_worker']
  const registerWorkerReal = iiiSubMembers['register_worker']
  const registerWorker = (registerWorkerReal?.docstring?.value ? registerWorkerReal : null)
    ?? (registerWorkerAlias?.docstring?.value ? registerWorkerAlias : null)
    ?? registerWorkerReal
    ?? registerWorkerAlias

  const methods: FunctionDoc[] = []
  if (iiiClass?.members) {
    for (const [name, member] of Object.entries(iiiClass.members)) {
      if (name.startsWith('_')) continue
      if (member.kind === 'function') {
        methods.push(griffeToFunction(member))
      }
    }
  }

  const loggerModule = rootMembers['logger']
  const loggerClass: GriffeObject | undefined = loggerModule?.members?.['Logger']
  let loggerSection: LoggerDoc | undefined
  if (loggerClass) {
    const loggerMethods: FunctionDoc[] = []
    if (loggerClass.members) {
      for (const [name, member] of Object.entries(loggerClass.members)) {
        if (name.startsWith('_')) continue
        if (member.kind === 'function') {
          loggerMethods.push(griffeToFunction(member))
        }
      }
    }
    const description = extractDocstring(loggerClass)
    if (description || loggerMethods.length > 0) {
      loggerSection = { description, methods: loggerMethods }
    }
  }

  const skipClasses = new Set(['III', 'Logger'])
  const types: TypeDoc[] = []

  for (const [, member] of Object.entries(rootMembers)) {
    if (member.kind === 'module' && member.members) {
      types.push(...extractTypesFromModule(member.members, skipClasses))
    } else if (member.kind === 'class' && !skipClasses.has(member.name)) {
      types.push(griffeToType(member))
    }
  }

  const entryFn = registerWorker
    ? griffeToFunction(registerWorker)
    : {
        name: 'register_worker',
        signature: '(address: str, options: InitOptions | None = None) -> III',
        description: 'Create an III client and auto-start its connection task.',
        params: [],
        returns: { type: 'III', description: '' },
        examples: [],
      }

  return {
    metadata: {
      language: 'python',
      languageLabel: 'Python',
      title: 'Python SDK',
      description: 'API reference for the iii SDK for Python.',
      installCommand: 'pip install iii-sdk',
      importExample: 'from iii import register_worker, InitOptions',
    },
    initialization: {
      entryPoint: entryFn,
    },
    methods,
    types,
    loggerSection,
  }
}

export function parseGriffe(jsonPath: string): SdkDoc {
  return parseGriffeData(JSON.parse(readFileSync(jsonPath, 'utf-8')))
}
