import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { runInNewContext } from 'node:vm'
import { parse, stringify } from 'yaml'
import { digest, normalizeLines, reviewedPicomatch, validatePicomatchPatches } from './security-patches.mjs'

const root = fileURLToPath(new URL('../', import.meta.url))

// Follow installed dependency links rather than pnpm's platform-dependent
// virtual-store directory names (which are shortened on Windows by default).
function resolveDependency(importer, chain) {
  let load = createRequire(path.join(root, importer, 'package.json'))
  let filename
  for (const name of chain) {
    filename = load.resolve(name)
    load = createRequire(filename)
  }
  return { filename, load }
}

test('patch integrity accepts CRLF but rejects unreviewed patches and snapshots', () => {
  const lock = readFileSync(path.join(root, 'pnpm-lock.yaml'), 'utf8')
  const readPatch = name => readFileSync(path.join(root, name), 'utf8')
  assert.deepEqual([...validatePicomatchPatches()], ['2.3.2', '4.0.7'])
  const quoted = stringify(parse(lock), { defaultKeyType: 'QUOTE_SINGLE', sortMapEntries: true })
  assert.equal(validatePicomatchPatches({ lock: quoted }).size, 2)
  assert.equal(validatePicomatchPatches({
    lock: lock.replace(/\r?\n/g, '\r\n'),
    readPatch: name => readPatch(name).replace(/\r?\n/g, '\r\n'),
  }).size, 2)
  const original = reviewedPicomatch['4.0.7'].patch
  const changed = name => `${readPatch(name)}\n// refreshed patch\n`
  const changedHash = digest(changed('patches/picomatch@4.0.7.patch'))
  assert.throws(() => validatePicomatchPatches({
    lock: lock.replaceAll(original, changedHash),
    readPatch: name => name.includes('4.0.7') ? changed(name) : readPatch(name),
  }), /Unreviewed/)
  const unpatched = parse(lock)
  unpatched.snapshots['picomatch@4.0.7'] = {}
  assert.throws(() => validatePicomatchPatches({
    lock: stringify(unpatched),
  }), /Unreviewed/)
  assert.throws(() => validatePicomatchPatches({
    lock: lock.replace(original, '0'.repeat(64)),
  }), /Unreviewed/)
})

const picomatchChains = {
  '2.3.2': ['website', ['shadcn', 'fast-glob', 'micromatch', 'picomatch']],
  '4.0.7': ['sdk/packages/node/helpers', ['tsdown', 'picomatch']],
}
for (const [version, [importer, chain]] of Object.entries(picomatchChains)) {
  test(`picomatch ${version}: reviewed snapshot and bounded repeated extglobs`, () => {
    validatePicomatchPatches()
    const { filename, load } = resolveDependency(importer, chain)
    const packageFile = path.join(path.dirname(filename), 'package.json')
    assert.equal(JSON.parse(readFileSync(packageFile, 'utf8')).version, version)
    assert.equal(
      digest(readFileSync(path.join(path.dirname(filename), 'lib/parse.js'), 'utf8')),
      reviewedPicomatch[version].parser,
      'The installed parser must be the reviewed patched snapshot',
    )
    const result = spawnSync(process.execPath, ['--input-type=commonjs', '-e', `
      const assert = require('node:assert/strict');
      const pm = require(${JSON.stringify(filename)});
      const patterns = ['+(*)', '*(*)', '+(*a)', '*(*a)', '+(*?)', '+(a*)', '+(+(*a))', '+(a+)', '+(*(a)a)', '+(*(a)ab)', '+(*(a)b|a)'];
      for (const pattern of patterns) {
        const regex = pm.makeRe(pattern);
        assert.equal(regex.test(pattern), true, pattern + ' must match its literal fallback');
        assert.equal(regex.test('a'.repeat(48) + '!'), false, pattern);
        assert.equal(regex.test('a'.repeat(48) + '/'), false, pattern);
      }
      assert.equal(pm.isMatch('+a+a', '+(+a)'), true);
      assert.equal(pm.makeRe('+(*(a)b)').test('aabab'), true);
      for (const options of [{ nocase: true }, { flags: 'i' }, { flags: 'gi' }, { nocase: true, flags: '' }]) {
        const pattern = '+(*(a)A)';
        const regex = pm.makeRe(pattern, options);
        assert.equal(regex.test(pattern), true);
        regex.lastIndex = 0;
        assert.equal(regex.test('a'.repeat(48) + '!'), false);
        const safe = pm.makeRe('+(*(a)b)', options);
        assert.equal(safe.test('AaBAb'), true);
      }
      assert.equal(pm.makeRe('+(*(a)A)').test('aaAaA'), true);
      assert.equal(pm.makeRe('+(*(a)A)', { nocase: true, flags: 'm' }).test('aaAaA'), true);
      assert.equal(pm.makeRe('+(+(a)b)').test('aabab'), true);
      assert.equal(pm.makeRe('*(*(a)b)').test('aabab'), true);
      for (const pattern of ['+(*(a)b)', '+(+(a)b)', '*(*(a)b)']) {
        assert.equal(pm.makeRe(pattern).test('a'.repeat(20000) + '!'), false);
      }
      assert.equal(pm.isMatch('item-+(+(*(a)))', '+(item-+(+(*(a))))'), true);
      assert.equal(pm.makeRe('a'.repeat(64), { maxLength: 64 }).test('a'.repeat(64)), true);
      assert.throws(() => pm.makeRe('a'.repeat(65), { maxLength: 64 }), /maximum allowed length/);
      for (const depth of [1, 2, 3, 4, 64]) {
        const pattern = '+('.repeat(depth) + 'a' + ')'.repeat(depth);
        const regex = pm.makeRe(pattern, { maxExtglobRecursion: 1 });
        assert.equal(regex.test('aaa'), depth === 1);
        if (depth > 1) assert.equal(regex.test(pattern), true);
      }
      assert.equal(pm.isMatch('a', '?(a)'), true);
      assert.equal(pm.isMatch('1+2', '+([[:digit:]+])', { posix: true, literalBrackets: false }), true);
      assert.equal(pm.isMatch('src/index.ts', '**/*.ts'), true);
      assert.equal(pm.isMatch('foofoo', '+(foo|bar)'), true);
      assert.equal(pm.isMatch('aaa', '+(*(a))'), true);
      assert.equal(pm.isMatch('***', '+(\\\\*)'), true);
      assert.equal(pm.isMatch('+++', '+([+])'), true);
      assert.equal(pm.isMatch('aaa', '+(a)'), true);
      assert.equal(pm.isMatch('file.ts', '!(*.js)'), true);
      assert.equal(pm.isMatch('aaa', '+(*a)', { maxExtglobRecursion: false }), true);
    `], { encoding: 'utf8', timeout: 3000 })
    assert.equal(result.error, undefined, result.error?.message)
    assert.equal(result.status, 0, result.stderr)
    assert.equal(typeof load('picomatch'), 'function')
  })
}

test('minimatch 11 preserves partial matching required by typedoc', () => {
  const { load } = resolveDependency('sdk/packages/node/iii', ['typedoc', 'minimatch'])
  const { Minimatch } = load('minimatch')
  assert.equal(new Minimatch('src/**/*.ts').match('src', true), true)
  assert.equal(new Minimatch('src/**/*.ts').match('src/index.ts'), true)
  assert.equal(new Minimatch('src/**/*.ts').match('src/index.js'), false)
  assert.equal(new Minimatch('!**/node_modules/**').match('src/index.ts'), true)
})

test('KaTeX renders math and does not trust unsafe HTML commands', () => {
  const { load } = resolveDependency('website', ['mermaid', 'katex'])
  const katex = load('katex')
  assert.match(katex.renderToString('c = \\pm\\sqrt{a^2 + b^2}'), /katex/)
  const html = katex.renderToString('\\href{javascript:alert(1)}{click}', {
    trust: false,
    strict: 'ignore',
  })
  assert.doesNotMatch(html, /href="javascript:/)
})

test('audit reports all advisories and fails closed when patch integrity fails', () => {
  const source = readFileSync(path.join(root, 'scripts/audit-sdk.mjs'), 'utf8')
  const crlf = source.replace(/\r?\n/g, '\r\n')
  const code = normalizeLines(crlf)
    .replace(/^#!.*\n/, '')
    .replace(/^import .*\n/gm, '')
  const advisories = {
    picomatch: {
      module_name: 'picomatch', severity: 'high',
      github_advisory_id: 'GHSA-fc5q-39xc-vjpf',
      findings: [{ version: '4.0.7', paths: ['sdk__packages__node__helpers>tsdown>picomatch'] }],
    },
    other: {
      module_name: 'another-package', severity: 'critical',
      github_advisory_id: 'GHSA-other',
      findings: [{ version: '1.0.0', paths: ['sdk__packages__node__iii>another-package'] }],
    },
  }
  for (const report of [advisories, {}]) {
    let audited = false
    let exitCode = 0
    const messages = []
    const errors = []
    runInNewContext(code, {
      validatePicomatchPatches() { throw new Error('invalid digest') },
      execFileSync() {
        audited = true
        const stdout = JSON.stringify({ advisories: report })
        if (Object.keys(report).length) {
          throw Object.assign(new Error('pnpm audit exited with status 1'), { stdout })
        }
        return stdout
      },
      console: { log: message => messages.push(message), error: message => errors.push(message) },
      process: { exit: code => { exitCode = code } },
    })
    assert.equal(audited, true)
    assert.equal(exitCode, 1)
    assert.match(errors.join('\n'), /invalid digest/)
    assert.doesNotMatch(messages.join('\n'), /Locally patched advisory paths/)
    if (Object.keys(report).length) {
      assert.match(errors.join('\n'), /another-package/)
      assert.match(errors.join('\n'), /picomatch@4\.0\.7.*sdk__packages__node__helpers>tsdown>picomatch/)
      assert.doesNotMatch(messages.join('\n'), /picomatch/)
    }
  }
})
