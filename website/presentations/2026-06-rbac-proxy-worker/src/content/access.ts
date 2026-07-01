/* access — the live access-resolution explorer (A9).
 *
 * This is the engine's decision flow (rbac_config.rs:253-289) and wildcard
 * matcher (rbac_config.rs:46-82) re-stated in TS so the toggle is faithful, not
 * a mockup: switch the session profile and every function id is re-evaluated by
 * the exact five-rule order the proxy vendors.
 */

export type MetaValue = string | number | boolean

export interface CatalogFn {
  id: string
  metadata?: Record<string, MetaValue>
}

export type ExposeFilter = { kind: 'match'; pattern: string } | { kind: 'metadata'; meta: Record<string, MetaValue> }

export interface AccessProfile {
  id: string
  label: string
  note: string
  allowed_functions: string[]
  forbidden_functions: string[]
  expose: ExposeFilter[]
}

/** the infrastructure carve-out — ten ids that stay reachable regardless of expose. */
export const CARVE_OUT: string[] = [
  'engine::channels::create',
  'engine::workers::register',
  'engine::log::info',
  'engine::log::warn',
  'engine::log::error',
  'engine::log::debug',
  'engine::log::trace',
  'engine::baggage::get',
  'engine::baggage::set',
  'engine::baggage::get_all',
]

/** the function ids shown as chips, with the metadata the proxy would learn from the catalog. */
export const ACCESS_CATALOG: CatalogFn[] = [
  { id: 'api::users::list' },
  { id: 'api::users::get' },
  { id: 'api::admin::delete' },
  { id: 'api::admin::purge' },
  { id: 'billing::charge' },
  { id: 'reports::public', metadata: { public: true } },
  { id: 'metrics::public', metadata: { public: true } },
  { id: 'internal::secrets::read' },
  { id: 'engine::channels::create' },
  { id: 'engine::log::info' },
  { id: 'engine::functions::list' },
  { id: 'engine::workers::list' },
]

export const ACCESS_PROFILES: AccessProfile[] = [
  {
    id: 'tenant',
    label: 'api tenant',
    note: 'a normal tenant: the api surface and anything public, with one hard deny.',
    allowed_functions: [],
    forbidden_functions: ['api::admin::purge'],
    expose: [
      { kind: 'match', pattern: 'api::*' },
      { kind: 'match', pattern: '*::public' },
    ],
  },
  {
    id: 'public',
    label: 'public only',
    note: 'no auth context to speak of: only functions tagged public: true are visible.',
    allowed_functions: [],
    forbidden_functions: [],
    expose: [{ kind: 'metadata', meta: { public: true } }],
  },
  {
    id: 'admin',
    label: 'admin',
    note: 'elevated: the api surface plus an explicit allow beyond expose, and read access to discovery.',
    allowed_functions: ['internal::secrets::read'],
    forbidden_functions: [],
    expose: [
      { kind: 'match', pattern: 'api::*' },
      { kind: 'match', pattern: '*::public' },
      { kind: 'match', pattern: 'engine::functions::*' },
    ],
  },
  {
    id: 'locked',
    label: 'locked down',
    note: 'nothing exposed at all; only the infrastructure carve-out keeps setup working.',
    allowed_functions: [],
    forbidden_functions: [],
    expose: [],
  },
]

export interface AccessVerdict {
  allowed: boolean
  rule: string
}

/** `*` matches any run of characters, anchored at both ends, case-sensitive. */
function wildcard(pattern: string, id: string): boolean {
  const escaped = pattern
    .split('*')
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
    .join('.*')
  return new RegExp(`^${escaped}$`).test(id)
}

function metadataMatches(fn: CatalogFn, meta: Record<string, MetaValue>): boolean {
  if (!fn.metadata) return false
  return Object.entries(meta).every(([k, v]) => fn.metadata?.[k] === v)
}

/** the five-rule access-resolution order, returning which rule decided it. */
export function resolveAccess(fn: CatalogFn, profile: AccessProfile): AccessVerdict {
  if (profile.forbidden_functions.includes(fn.id)) {
    return { allowed: false, rule: 'rule 1 · forbidden' }
  }
  if (profile.allowed_functions.includes(fn.id)) {
    return { allowed: true, rule: 'rule 2 · allowed' }
  }
  if (CARVE_OUT.includes(fn.id)) {
    return { allowed: true, rule: 'rule 3 · carve-out' }
  }
  for (const filter of profile.expose) {
    if (filter.kind === 'match' && wildcard(filter.pattern, fn.id)) {
      return { allowed: true, rule: `rule 4 · match("${filter.pattern}")` }
    }
    if (filter.kind === 'metadata' && metadataMatches(fn, filter.meta)) {
      return { allowed: true, rule: 'rule 4 · metadata filter' }
    }
  }
  return { allowed: false, rule: 'rule 5 · not exposed' }
}

/** render an expose filter back to its config form. */
export function filterLabel(filter: ExposeFilter): string {
  if (filter.kind === 'match') return `match("${filter.pattern}")`
  const pairs = Object.entries(filter.meta).map(([k, v]) => `${k}: ${v}`)
  return `metadata { ${pairs.join(', ')} }`
}
