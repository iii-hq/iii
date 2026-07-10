/* rbac — the contract, for the rbac-contract deep dive (rbac.md). */

export interface FieldRow {
  name: string
  type: string
  desc: string
}

export const AUTH_INPUT_FIELDS: FieldRow[] = [
  {
    name: 'headers',
    type: 'Record<string, string>',
    desc: 'http headers from the upgrade request, e.g. authorization.',
  },
  { name: 'query_params', type: 'Record<string, string[]>', desc: 'query params; repeated keys preserved.' },
  { name: 'ip_address', type: 'string', desc: 'connecting client ip as the proxy sees it.' },
]

export const AUTH_RESULT_FIELDS: FieldRow[] = [
  { name: 'allowed_functions', type: 'string[] = []', desc: 'allow beyond expose_functions.' },
  { name: 'forbidden_functions', type: 'string[] = []', desc: 'deny even if exposed. highest precedence.' },
  {
    name: 'allowed_trigger_types',
    type: 'string[] | omitted',
    desc: 'trigger types the worker may bind. omitted = all.',
  },
  { name: 'allow_trigger_type_registration', type: 'boolean = false', desc: 'may register new trigger types.' },
  { name: 'allow_function_registration', type: 'boolean = true', desc: 'may register new functions.' },
  { name: 'function_registration_prefix', type: 'string | omitted', desc: 'private namespace prefix.' },
  { name: 'context', type: 'object = {}', desc: 'forwarded to middleware + hooks.' },
]

export const MIDDLEWARE_FIELDS: FieldRow[] = [
  { name: 'function_id', type: 'string', desc: 'the function the worker wants to invoke.' },
  { name: 'payload', type: 'object', desc: 'the payload the worker sent.' },
  { name: 'action', type: 'TriggerAction | omitted', desc: 'enqueue / void, if any.' },
  { name: 'context', type: 'object', desc: 'auth result context for this session.' },
]

export interface HookRow {
  hook: string
  firesOn: string
  result: string
}

export const HOOK_ROWS: HookRow[] = [
  {
    hook: 'on_function_registration',
    firesOn: 'RegisterFunction',
    result: '{ function_id?, description?, metadata? }',
  },
  {
    hook: 'on_trigger_registration',
    firesOn: 'RegisterTrigger',
    result: '{ trigger_id?, trigger_type?, function_id?, config? }',
  },
  {
    hook: 'on_trigger_type_registration',
    firesOn: 'RegisterTriggerType',
    result: '{ trigger_type_id?, description? }',
  },
]

export const RESOLUTION_RULES = [
  { n: '1', text: 'function_id in forbidden_functions', verdict: 'deny' },
  { n: '2', text: 'function_id in allowed_functions', verdict: 'allow' },
  { n: '3', text: 'function_id in the infrastructure carve-out', verdict: 'allow' },
  { n: '4', text: 'any expose_functions filter matches', verdict: 'allow' },
  { n: '5', text: 'otherwise', verdict: 'deny' },
] as const

export interface Divergence {
  title: string
  engine: string
  proxy: string
  why: string
}

export const DIVERGENCES: Divergence[] = [
  {
    title: 'self-invoke under a prefix',
    engine: 'a prefixed worker invoking its own bare foo gets NOT_FOUND.',
    proxy: 'resolves the bare id to the session’s own {prefix}::foo so the call succeeds.',
    why: 'a worker should be able to call what it registered. opt-out for strict parity.',
  },
  {
    title: 'denied-discovery error code',
    engine: 'distinguishes FORBIDDEN (denied) from NOT_FOUND (missing), leaking existence.',
    proxy: 'defaults to FORBIDDEN parity; may opt into collapsing denied → NOT_FOUND.',
    why: 'hardening for hostile multi-tenant discovery.',
  },
  {
    title: 'registration-denial frame',
    engine: 'denies a trigger/type registration silently, with no result frame.',
    proxy: 'replies TriggerRegistrationResult{ error: REGISTRATION_DENIED } so the worker learns why.',
    why: 'a silent drop is hard to debug; the sdk already handles the frame.',
  },
  {
    title: 'trigger target access',
    engine: 'gates RegisterTrigger only by trigger-type perms + the optional hook.',
    proxy: 'also resolves the bound function_id and runs access resolution on it.',
    why: 'trigger firing bypasses the invoke gate; without this a worker binds to a function it cannot call.',
  },
]
