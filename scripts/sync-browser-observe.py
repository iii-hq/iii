#!/usr/bin/env python3
"""Keep the browser SDK's observe code in step with the Node SDK, its reference.

    scripts/sync-browser-observe.py           # rewrite the derived files
    scripts/sync-browser-observe.py --check   # fail (exit 1) when one differs

The browser SDK (`sdk/packages/node/iii-browser`) imports the Node SDK's
filter evaluator (`src/filter`) directly. The rest of observe cannot be
imported: it uses Node APIs (`Buffer`, `timer.unref()`, `AsyncLocalStorage`) and
Node's types. So the browser's copies of the host side, the reporter, the wire
format, the observer handle and the public types, and of the observe unit and
integration tests, are *derived*: each one is the Node file plus the short,
declared list of patches below, under a one-line header saying so. To change
one, change the Node file or its patches here and run this script; CI runs
`--check`.

Every patch names the text it replaces and how many times it must occur, so a
Node change that moves an anchor fails here instead of drifting silently.

Not derived (browser-specific): `src/iii.ts`, `src/invocation-context.ts`
(a synchronous scope instead of `AsyncLocalStorage`), `tests/observe-context.test.ts`,
`tests/observe-fake-engine.ts` and `tests/integration/observe-utils.ts`.
"""

import difflib
import fnmatch
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
NODE = 'sdk/packages/node/iii'
BROWSER = 'sdk/packages/node/iii-browser'
SCRIPT = 'scripts/sync-browser-observe.py'

ANY = -1  # a patch that may apply any number of times, including none


def header(source: str) -> str:
    return f'// Derived from {source} by {SCRIPT}; edit those, not this file.\n'


def apply(source: str, patches) -> str:
    text = (REPO / source).read_text()
    for old, new, count in patches:
        if isinstance(old, re.Pattern):
            text, n = old.subn(new, text)
        else:
            n = text.count(old)
            text = text.replace(old, new)
        if count != ANY and n != count:
            sys.exit(f'{SCRIPT}: {source}: expected {count} x {old!r}, found {n}')
    return text


# ── what is derived, and what is not ────────────────────────────────────────
#
# Error shapes: the browser SDK keeps its pre-0.25 rejections, so the derived
# tests assert them instead of Node's (the parity note lives here):
#   - a handler or a deliver hook that throws (remote or local call): the
#     engine's error body, a plain `{ code: 'invocation_failed', message,
#     stacktrace }` object (Node: an `InvocationError` whose message is
#     `invocation_failed: <message>`);
#   - a call that times out (remote or local): an `Error` with the message
#     `Invocation timeout after <ms>ms: <function_id>` and `code: 'TIMEOUT'`,
#     `function_id` (Node: an `InvocationError` whose message is
#     `TIMEOUT: invocation timed out after <ms>ms`);
#   - errors the SDK raises itself (`LOCAL_DEPTH_EXCEEDED`, browser only): an
#     `InvocationError`, as in Node.
#
# The invocation context is a synchronous scope in the browser
# (`src/invocation-context.ts`): a handler or a deliver hook sees it up to its
# first `await`. The derived tests read `currentInvocation()` before awaiting.
#
# Every Node file that matches OBSERVE_FILES is either derived (MODULES, TESTS)
# or listed in EXCLUDED with the reason; `--check` fails on one that is neither.

OBSERVE_FILES = [  # recursive and loose: a new or moved Node file (`src/reporter-extra.ts`) is still found
    'src/**/*observ*.ts',
    'src/**/*reporter*.ts',
    'src/**/*invocation*.ts',
    'src/**/*filter*.ts',
    'src/filter/**/*.ts',
    'tests/**/*observe*.ts',
    'tests/**/*filter*.ts',
]

EXCLUDED = {
    'src/invocation-context.ts': 'browser-specific: a synchronous scope, no AsyncLocalStorage',
    'src/filter/**': 'imported by the browser SDK (src/filter.ts), not derived',
    'tests/observe-fake-engine.ts': "the browser's fake engine replaces the global WebSocket instead of mocking `ws`",
    'tests/observe-e2e-utils.ts': 'browser helpers live in tests/integration/observe-utils.ts',
    'tests/observe-local-span.test.ts': 'OpenTelemetry spans: the browser SDK has none',
}

# ── source modules ───────────────────────────────────────────────────────────

MODULES = {
    'observer-handle.ts': [],
    'observe-wire.ts': [
        ('  InvokeFunctionMessage,\n  JsonValue,\n', '  InvokeFunctionMessage,\n', 1),
        ('msg.metadata = v.metadata as JsonValue', 'msg.metadata = v.metadata', 1),
    ],
    'reporter.ts': [
        (
            "import { MessageType } from './iii-types'\n",
            "import { MessageType } from './iii-types'\nimport { unrefTimer, utf8ByteLength } from './utils'\n",
            1,
        ),
        ("Buffer.byteLength(s, 'utf8')", 'utf8ByteLength(s)', 1),
        ('this.statsTimer.unref?.()', 'unrefTimer(this.statsTimer)', 1),
        ('this.lingerTimer.unref?.()', 'unrefTimer(this.lingerTimer)', 1),
    ],
    'observable.ts': [
        (
            "import type { Reporter, ReportInit, ReportValues } from './reporter'\n",
            "import type { Reporter, ReportInit, ReportValues } from './reporter'\nimport { unrefTimer } from './utils'\n",
            1,
        ),
        ('timer.unref?.()', 'unrefTimer(timer)', 1),
        # The browser's error bodies of a failure (src/errors.ts). A handler's:
        # an `Error` from any realm, an engine error body rethrown, anything
        # else; it never throws. A deliver hook's also keeps the code of the
        # browser's own rejections: an engine error body and a timed-out call.
        (
            "import { InvocationError, ObserveRejection } from './errors'\n",
            "import { deliverErrorBody, ObserveRejection } from './errors'\n",
            1,
        ),
        (
            """/** The error a handler's failure becomes, the same on the local and the remote path. */
export function handlerErrorBody(error: unknown): ErrorBody {
  const isError = error instanceof Error
  return {
    code: 'invocation_failed',
    message: isError ? error.message : String(error),
    stacktrace: isError ? error.stack : undefined,
  }
}

/**
 * The error a deliver hook's failure becomes: an {@link ObserveRejection} or
 * an {@link InvocationError} (a call it made that failed, rethrown) keeps its
 * code and message; anything else is a handler failure (`invocation_failed`).
 */
export function deliverErrorBody(error: unknown): ErrorBody {
  if (error instanceof ObserveRejection) return { code: error.code, message: error.message }
  if (error instanceof InvocationError) {
    const prefix = `${error.code}: `
    return {
      code: error.code,
      message: error.message.startsWith(prefix) ? error.message.slice(prefix.length) : error.message,
      stacktrace: error.stacktrace,
    }
  }
  return handlerErrorBody(error)
}

""",
            '',
            1,
        ),
    ],
    'observe-types.ts': [
        (
            """ * `obs.matches(call)`. It runs inside the function's invocation, so the calls
 * it makes have that invocation as their caller.
 */""",
            """ * `obs.matches(call)`. It runs inside the function's invocation, so the calls
 * it makes have that invocation as their caller.
 *
 * Browsers have no `AsyncLocalStorage`: the hook runs in the invocation's
 * scope up to its first `await`. A call it makes after one still has the
 * invocation as its caller while that invocation is the only one running on
 * the page; to be sure, read `iii.currentInvocation()` first and re-enter it
 * with `iii.withInvocation(inv, fn)`. A call through an observer is routed to
 * the observer's target either way.
 */""",
            1,
        ),
        (
            """/**
 * The invocation the current code runs in, in every handler, remote or
 * local (`iii.currentInvocation()`).
 */""",
            """/**
 * The invocation the current code runs in, in every handler, remote or
 * local (`iii.currentInvocation()`).
 *
 * Browsers have no `AsyncLocalStorage`, so the SDK knows it only while the
 * handler runs synchronously: up to its first `await`. To keep it across an
 * `await`, read it first and re-enter it with `iii.withInvocation(inv, fn)`.
 */""",
            1,
        ),
        (
            """  activation?: { observer_id: string; activation_id?: string }
  observe_depth: number
}""",
            """  activation?: { observer_id: string; activation_id?: string }
  observe_depth: number
  /**
   * The W3C `traceparent` the invocation arrived with (or was called with,
   * for a local call). Calls made inside the handler forward it. Browser SDK
   * only: there is no OpenTelemetry context to carry it.
   */
  traceparent?: string
  /** The W3C `baggage` that came with `traceparent`. */
  baggage?: string
}""",
            1,
        ),
    ],
}

# ── tests ────────────────────────────────────────────────────────────────────

MOCK_PRELUDE = re.compile(
    r"vi\.mock\('ws', async \(\) => \(await import\('\./observe-fake-engine'\)\)\.wsModule\(\)\)\n"
    r'vi\.resetModules\(\)\n'
    r"((?:const .* = await import\('[^']+'\)\n)+)"
)


GENERATED_IMPORTS = '// <generated imports>\n'


def static_imports(m: re.Match) -> str:
    """`const { a, b } = await import('../src/x')` lines → static imports.

    They are written where the mock was, behind a marker that
    `organize_imports` replaces by merging them into the file's first import
    block, in the order Biome's organizer keeps them in.
    """
    lines = []
    for line in m.group(1).strip().split('\n'):
        mm = re.match(r"const (\{[^}]*\}|\w+) = await import\('([^']+)'\)", line)
        names, path = mm.group(1), mm.group(2)
        if path == './observe-fake-engine':
            lines.append("import * as fake from './observe-fake-engine'")
        elif names.startswith('{'):
            specifiers = sorted((n.strip() for n in names[1:-1].split(',') if n.strip()), key=str.lower)
            lines.append(f"import {{ {', '.join(specifiers)} }} from '{path}'")
        else:
            lines.append(f"import * as {names} from '{path}'")
    return GENERATED_IMPORTS + ''.join(f'{line}\n' for line in lines)


IMPORT_LINE = re.compile(r"^import (?:type )?.* from '([^']+)'$")


def import_order(line: str):
    """Biome's order: packages first (as written), then paths, farthest first, by path."""
    source = IMPORT_LINE.match(line).group(1)
    if not source.startswith('.'):
        return (0, 0, '')
    return (1, -source.count('../'), source.lower())


def organize_imports(text: str) -> str:
    if GENERATED_IMPORTS not in text:
        return text
    before, after = text.split(GENERATED_IMPORTS, 1)
    generated = []
    rest = after.split('\n')
    while rest and IMPORT_LINE.match(rest[0]):
        generated.append(rest.pop(0))
    after = '\n'.join(rest).lstrip('\n')
    lines = before.split('\n')
    first = next(i for i, line in enumerate(lines) if line.startswith('import '))
    last = first
    while last < len(lines) and IMPORT_LINE.match(lines[last]):
        last += 1
    block = sorted(lines[first:last] + generated, key=import_order)
    head = '\n'.join(lines[:first] + block)
    tail = '\n'.join(lines[last:]).strip('\n')
    return f'{head}\n\n{tail}\n{after}' if tail else f'{head}\n\n{after}'


# Every unit test: static imports instead of the `ws` module mock (the browser
# fake replaces the global `WebSocket` before each test), the browser client
# type, and no Node-only options (OpenTelemetry, worker metrics).
UNIT = [
    (MOCK_PRELUDE, static_imports, 1),
    ('IIIClient', 'ISdk', ANY),
    (re.compile(r'^\s*otel: \{ enabled: false \},\n', re.M), '', ANY),
    (re.compile(r'^\s*enableMetricsReporting: false,\n', re.M), '', ANY),
    (', { otel: { enabled: false }, enableMetricsReporting: false })', ')', ANY),
]


def drop_unused_vi(text: str) -> str:
    """`vi` was only needed for the `ws` module mock in some files."""
    if 'vi.' in text:
        return text
    return re.sub(r"(import \{[^}]*?), vi( \} from 'vitest')", r'\1\2', text)


FILTER_CONFORMANCE = [
    (
        "import { compileFilter, FilterError } from '../src/filter'\n",
        "import * as nodeFilter from '../../iii/src/filter'\nimport { compileFilter, FilterError } from '../src/filter'\n",
        1,
    ),
    (
        "describe('filter conformance vectors', () => {",
        """/**
 * The browser SDK imports the Node SDK's evaluator instead of copying it, so
 * this runs the same code the Node SDK's own conformance test runs, here under
 * the browser package's build settings and in a DOM environment.
 */
describe('the shared evaluator', () => {
  it('is the Node SDK module, not a copy', () => {
    expect(compileFilter).toBe(nodeFilter.compileFilter)
    expect(FilterError).toBe(nodeFilter.FilterError)
  })
})

describe('filter conformance vectors', () => {""",
        1,
    ),
]

HOOKS = UNIT + [
    # The browser client has no `getConnectionState()`: the fake engine reads the state.
    ('sdk.getConnectionState()', 'fake.connectionState(sdk)', 1),
]

DELIVER = UNIT + [
    (
        """    await expect(sdk.call({ function_id: 'http::on-request', payload: { path: '/boom' } })).rejects.toMatchObject({
      code: 'invocation_failed',
      message: 'invocation_failed: no route',
      function_id: 'http::on-request',
    })""",
        """    // The browser SDK rejects with the error body the remote path gives.
    await expect(sdk.call({ function_id: 'http::on-request', payload: { path: '/boom' } })).rejects.toEqual({
      code: 'invocation_failed',
      message: 'no route',
      stacktrace: expect.any(String),
    })""",
        1,
    ),
    (
        """    await expect(response).rejects.toMatchObject({
      code: 'OBSERVER_NOT_ACTIVE',
      message: 'OBSERVER_NOT_ACTIVE: observer users is not active',
      function_id: 'http::on-request',
    })""",
        """    // The browser SDK rejects with the error body the remote path gives.
    await expect(response).rejects.toEqual({ code: 'OBSERVER_NOT_ACTIVE', message: 'observer users is not active' })""",
        1,
    ),
    (
        """    ).rejects.toMatchObject({
      code: 'OBSERVER_NOT_ACTIVE',
      message: 'OBSERVER_NOT_ACTIVE: subscriber removed',
      function_id: 'queue::on-message',
    })""",
        """    ).rejects.toEqual({ code: 'OBSERVER_NOT_ACTIVE', message: 'subscriber removed' }) // the remote path's error body""",
        1,
    ),
    (
        """      .onObserverDeliver(async (call) => {
        await Promise.resolve()
        inHook = sdk.currentInvocation()
        void sdk.call({ function_id: 'audit::log', payload: {}, timeoutMs: 50 }).catch(() => undefined)""",
        """      .onObserverDeliver(async (call) => {
        // Browser SDK: the scope ends at the hook's first `await`. A call made
        // after it still has the invocation as its caller: the only one running,
        // and delivering.
        inHook = sdk.currentInvocation()
        await Promise.resolve()
        void sdk.call({ function_id: 'audit::log', payload: {}, timeoutMs: 50 }).catch(() => undefined)""",
        1,
    ),
]

LOCAL_DISPATCH = UNIT + [
    ("import { InvocationError } from '../src/errors'\n", '', 1),
    (
        "import { CallAction, registerWorker } from '../src/index'\n",
        "import { CallAction, registerWorker } from '../src/index'\nimport { utf8ByteLength } from '../src/utils'\n",
        1,
    ),
    ('Buffer.byteLength(', 'utf8ByteLength(', ANY),
    (
        """  it('never runs an HTTP-invoked function locally', async () => {
    const { sdk, socket } = worker()
    sdk.registerFunction('ext::lambda', { url: 'https://example.invalid' })
    socket.open()
    socket.deliver(functionRegistered('ext::lambda'))
    socket.clear()
    void sdk.call({ function_id: 'ext::lambda', payload: {}, timeoutMs: 20 }).catch(() => undefined)
    expect(invokes(socket)).toHaveLength(1)
  })

""",
        '',
        1,
    ),
    (
        "  it('a failing handler rejects with the InvocationError the remote path gives', async () => {",
        "  it('a failing handler rejects with the error body the remote path gives', async () => {",
        1,
    ),
    (
        """    for (const e of [local, remoteError]) {
      expect(e).toBeInstanceOf(InvocationError)
      expect(e.code).toBe('invocation_failed')
      expect(e.message).toBe('invocation_failed: boom')
      expect(typeof e.stacktrace).toBe('string')
    }
    expect(local.function_id).toBe('err::local')
  })""",
        """    // The browser SDK rejects a failed call with the engine's error body, so
    // the local path rejects with the same body (the handler's).
    for (const e of [local, remoteError]) {
      expect(Object.keys(e).sort()).toEqual(['code', 'message', 'stacktrace'])
      expect(e.code).toBe('invocation_failed')
      expect(e.message).toBe('boom')
      expect(typeof e.stacktrace).toBe('string')
    }
  })""",
        1,
    ),
    (
        """    await expect(sdk.call({ function_id: 'slow::fn', payload: {}, timeoutMs: 30 })).rejects.toMatchObject({
      code: 'TIMEOUT',
      message: 'TIMEOUT: invocation timed out after 30ms',
      function_id: 'slow::fn',
    })""",
        """    // The browser SDK's timeout, on both paths: an Error with this message, a `code` and the `function_id`.
    const timedOut = { code: 'TIMEOUT', message: 'Invocation timeout after 30ms: slow::fn', function_id: 'slow::fn' }
    await expect(sdk.call({ function_id: 'slow::fn', payload: {}, timeoutMs: 30 })).rejects.toMatchObject(timedOut)
    await expect(
      sdk.call({ function_id: 'slow::fn', payload: {}, timeoutMs: 30, dispatch: 'remote' }),
    ).rejects.toMatchObject(timedOut)""",
        1,
    ),
    # Decision 22: only the Node SDK freezes a private function's values.
    (
        """  it("freezes both when NODE_ENV is 'development', so a write throws", async () => {
    const { sdk, seen, result } = await source('development')
    const payload = { list: [1, 2], at: new Date(0) }
    const out = await sdk.call<typeof payload, { list: number[] }>({ function_id: 'cron::on-tick', payload })
    expect(seen.input).toBe(payload)
    expect(out).toBe(result)
    expect(() => payload.list.push(3)).toThrow(TypeError)
    expect(() => out.list.push(3)).toThrow(TypeError)
    // a class instance is left alone
    expect(Object.isFrozen(payload.at)).toBe(false)
  })""",
        """  it("never freezes them, also when NODE_ENV is 'development' (decision 22: only the Node SDK does)", async () => {
    const { sdk, seen, result } = await source('development')
    const payload = { list: [1, 2] }
    const out = await sdk.call<typeof payload, { list: number[] }>({ function_id: 'cron::on-tick', payload })
    expect(seen.input).toBe(payload)
    expect(out).toBe(result)
    expect(Object.isFrozen(payload)).toBe(false)
    expect(Object.isFrozen(payload.list)).toBe(false)
    expect(Object.isFrozen(out)).toBe(false)
  })""",
        1,
    ),
]

PROTOCOL_FIXTURES = UNIT + [
    (
        'function connect(opts: { workerName?: string; namespace?: string; telemetry?: Json } = {}) {',
        'function connect(opts: { workerName?: string; namespace?: string } = {}) {',
        1,
    ),
    (
        re.compile(
            r'    const data = \{ \.\.\.\(frame\.data as Json\) \}\n.*?'
            r"    expect\(data\.features\)\.toEqual\(\['observe\.v1', 'observe\.v2', 'local-dispatch\.v1', 'large-messages\.v1'\]\)\n",
            re.S,
        ),
        """    const data = frame.data as Json
    const want = expected.data as Json
    // The frame is the fixture's; its payload is a subset of the fixture's
    // fields (a browser has no version, pid or telemetry), and `os` is the
    // user agent.
    expect({ ...frame, data: null }).toEqual({ ...expected, data: null })
    expect(Object.keys(data).filter((k) => !(k in want))).toEqual([])
    expect(data).toMatchObject({ runtime: 'browser', name: 'billing', namespace: 'default' })
    // The fixture is a native SDK's. A browser receives one 16 MiB frame at
    // most, so it declares every feature but `large-messages.v1`.
    expect(want.features).toEqual(['observe.v1', 'observe.v2', 'local-dispatch.v1', 'large-messages.v1'])
    expect(data.features).toEqual(['observe.v1', 'observe.v2', 'local-dispatch.v1'])
    expect(typeof data.os).toBe('string')
""",
        1,
    ),
    ("      telemetry: { language: 'en-US', project_name: 'billing', framework: 'iii-node' },\n", '', 1),
]

REPORTER = [
    (
        re.compile(r"(import \{ Reporter\b[^}]*\} from '\.\./src/reporter'\n)"),
        r"\1import { utf8ByteLength } from '../src/utils'\n",
        1,
    ),
    ('Buffer.byteLength(', 'utf8ByteLength(', ANY),
]

# Integration (a real engine at III_URL): Node's e2e scenarios with the
# browser's helpers (the browser client has no `getConnectionState()`).
E2E = [
    ("from './observe-e2e-utils'", "from './observe-utils'", 1),
    ("'../src/", "'../../src/", ANY),
    (re.compile(r'(\w+)\.getConnectionState\(\)'), r'connectionState(\1)', ANY),
]

E2E_LOCAL = E2E + [('\n  dropConnection,', '\n  connectionState,\n  dropConnection,', 1)]

E2E_ROUTED = E2E + [
    (
        "import { closeWorkers, collector, dropConnection, frameCounter, sleep, waitFor, worker } from './observe-utils'",
        """import {
  closeWorkers,
  collector,
  connectionState,
  dropConnection,
  frameCounter,
  sleep,
  traceId,
  waitFor,
  worker,
} from './observe-utils'""",
        1,
    ),
    (
        re.compile(r'\n\}\)\n$'),
        r"""

  it('the loop guard holds for a target that awaits before it calls back', async () => {
    // Browser SDK: the invocation scope ends at the first `await`. The call made
    // after it carries the depth of the activation still running, so the engine
    // stops the loop at max_observe_depth (8) all the same.
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::await-loop-${RUN}`, async (n: number) => n)
    const depths: number[] = []
    watcher.registerFunction(`e2e::on-await-loop-${RUN}`, async (envelope: { input: number }) => {
      depths.push(watcher.currentInvocation()?.observe_depth ?? -1)
      await sleep(5)
      await watcher.call({ function_id: `e2e::await-loop-${RUN}`, payload: envelope.input + 1 })
      return null
    })
    const handle = watcher.observe(
      { function_id: `e2e::await-loop-${RUN}`, capture: ['input'] },
      `e2e::on-await-loop-${RUN}`,
    )
    await handle.ready()
    await watcher.call({ function_id: `e2e::await-loop-${RUN}`, payload: 0 })
    await waitFor(() => depths.length >= 8, 'eight hops')
    await sleep(1000)
    expect(depths).toEqual([1, 2, 3, 4, 5, 6, 7, 8])
  })

  it('call sends metadata and traceparent: filters read metadata.*, and the activation continues the trace', async () => {
    // Browser SDK: there is no OpenTelemetry, so the trace context is what call() is given.
    const owner = worker('routed-owner')
    const watcher = worker('routed-watcher')
    owner.registerFunction(`e2e::tag-${RUN}`, async (input: { n: number }) => input.n)
    const got = collector(watcher, `e2e::on-tag-${RUN}`)
    const handle = watcher.observe(
      {
        function_id: `e2e::tag-${RUN}`,
        filters: { 'metadata.tenant': { $eq: 'acme' } },
        capture: ['input', 'metadata'],
      },
      `e2e::on-tag-${RUN}`,
    )
    await handle.ready()
    const traceparent = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01'
    await watcher.call({
      function_id: `e2e::tag-${RUN}`,
      payload: { n: 1 },
      metadata: { tenant: 'acme' },
      traceparent,
    })
    await watcher.call({ function_id: `e2e::tag-${RUN}`, payload: { n: 2 }, metadata: { tenant: 'other' } })
    await waitFor(() => got.length >= 1, 'the activation')
    await sleep(200)
    expect(got).toHaveLength(1)
    expect(got[0].data).toMatchObject({ input: { n: 1 }, metadata: { tenant: 'acme' } })
    expect(traceId(got[0].inv?.traceparent)).toBe(traceId(traceparent))
  })
})
""",
        1,
    ),
]

# Pending until hosted, host restart and standby takeover, against a real
# engine: the browser's integration helpers and ids of its own.
FAULT_TOLERANCE = [
    ("'../src/", "'../../src/", ANY),
    ('IIIClient', 'ISdk', ANY),
    ('-node-${RUN}', '-browser-${RUN}', 3),
]

TESTS = {
    'filter-conformance.test.ts': ('filter-conformance.test.ts', FILTER_CONFORMANCE),
    'observe-observer.test.ts': ('observe-observer.test.ts', UNIT),
    'observe-hooks.test.ts': ('observe-hooks.test.ts', HOOKS),
    'observe-deliver.test.ts': ('observe-deliver.test.ts', DELIVER),
    'observe-local-dispatch.test.ts': ('observe-local-dispatch.test.ts', LOCAL_DISPATCH),
    'observe-reporter.test.ts': ('observe-reporter.test.ts', REPORTER),
    'observe-protocol-fixtures.test.ts': ('observe-protocol-fixtures.test.ts', PROTOCOL_FIXTURES),
    'observe-e2e-routed.test.ts': ('integration/observe-routed.test.ts', E2E_ROUTED),
    'observe-e2e-local.test.ts': ('integration/observe-local.test.ts', E2E_LOCAL),
    'observe-fault-tolerance.test.ts': ('integration/observe-fault-tolerance.test.ts', FAULT_TOLERANCE),
}


def derived():
    """(target path, derived text) for every derived file."""
    for name, patches in MODULES.items():
        source = f'{NODE}/src/{name}'
        yield f'{BROWSER}/src/{name}', header(source) + apply(source, patches)
    for name, (target, patches) in TESTS.items():
        source = f'{NODE}/tests/{name}'
        yield f'{BROWSER}/tests/{target}', header(source) + organize_imports(drop_unused_vi(apply(source, patches)))


def unaccounted() -> list:
    """Node observe files that are neither derived nor EXCLUDED."""
    node = REPO / NODE
    derived_sources = {f'src/{name}' for name in MODULES} | {f'tests/{name}' for name in TESTS}
    out = []
    found = sorted({path.relative_to(node).as_posix() for pattern in OBSERVE_FILES for path in node.glob(pattern)})
    for rel in found:
        if rel in derived_sources or rel in EXCLUDED:
            continue
        # `*` in an EXCLUDED pattern also matches `/` (fnmatch), so `dir/**` covers the whole tree.
        if any(fnmatch.fnmatchcase(rel, p) for p in EXCLUDED if '*' in p):
            continue
        out.append(rel)
    return out


def main() -> int:
    check = '--check' in sys.argv[1:]
    missing = unaccounted()
    if missing:
        sys.stderr.write(
            f'{SCRIPT}: Node observe files neither derived nor EXCLUDED: {", ".join(missing)}.\n'
            'Derive them (MODULES / TESTS) or list them in EXCLUDED with the reason.\n'
        )
        return 1
    stale = []
    for target, text in derived():
        path = REPO / target
        current = path.read_text() if path.exists() else None
        if current == text:
            continue
        if check:
            stale.append(target)
            diff = difflib.unified_diff(
                (current or '').splitlines(keepends=True),
                text.splitlines(keepends=True),
                f'{target} (committed)',
                f'{target} (derived)',
                n=1,
            )
            sys.stderr.write(''.join(list(diff)[:60]))
        else:
            path.write_text(text)
            print(f'wrote {target}')
    if stale:
        sys.stderr.write(
            f'\n{len(stale)} derived browser SDK file(s) differ from the Node SDK + {SCRIPT} patches.\n'
            f'Run `python3 {SCRIPT}` and commit the result (or adjust the patches there).\n'
        )
        return 1
    if check:
        print('derived browser SDK observe files are up to date')
    return 0


if __name__ == '__main__':
    sys.exit(main())
