export interface CatalogFn {
  id: string
  worker: string
}

/** a representative slice of a real iii registry catalog */
export const CATALOG: CatalogFn[] = [
  { id: 'harness::send', worker: 'harness' },
  { id: 'harness::cancel', worker: 'harness' },
  { id: 'harness::status', worker: 'harness' },
  { id: 'session-manager::append', worker: 'session-manager' },
  { id: 'session-manager::get', worker: 'session-manager' },
  { id: 'llm-router::route', worker: 'llm-router' },
  { id: 'llm-router::models::list', worker: 'llm-router' },
  { id: 'context-manager::assemble', worker: 'context-manager' },
  { id: 'storage::get', worker: 'storage' },
  { id: 'storage::put', worker: 'storage' },
  { id: 'storage::list', worker: 'storage' },
  { id: 'storage::delete', worker: 'storage' },
  { id: 'email::send', worker: 'email' },
  { id: 'email::accounts::list', worker: 'email' },
  { id: 'database::query', worker: 'database' },
  { id: 'coder::read', worker: 'coder' },
  { id: 'coder::search', worker: 'coder' },
]

export interface Scope {
  value: string
  label: string
  /** glob written into codegen.yml's `functions:` list */
  glob: string
}

export const SCOPES: Scope[] = [
  { value: 'harness', label: 'one worker', glob: 'harness::*' },
  { value: 'storage', label: 'another worker', glob: 'storage::*' },
  { value: 'all', label: 'the whole registry', glob: '*' },
]

/** does a function id fall inside a scope's glob? */
export function inScope(fnId: string, glob: string): boolean {
  if (glob === '*') return true
  const prefix = glob.replace(/\*$/, '')
  return fnId.startsWith(prefix)
}
