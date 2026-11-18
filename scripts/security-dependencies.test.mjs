import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readFileSync, readdirSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const store = path.join(root, 'node_modules/.pnpm')
const packages = readdirSync(store)

function requirePackage(name, version) {
  const lock = readFileSync(path.join(root, 'pnpm-lock.yaml'), 'utf8')
  const patch = name === 'picomatch'
    ? lock.split(`  picomatch@${version}:\n    hash: `)[1]?.split('\n')[0]
    : undefined
  const entries = packages.filter(entry => patch
    ? entry === `${name}@${version}_patch_hash=${patch}`
    : entry.startsWith(`${name}@${version}`))
  assert.ok(entries.length > 0, `${name}@${version} must be installed`)
  return entries.map(entry => {
    const filename = path.join(store, entry, 'node_modules', name, 'package.json')
    return { filename, load: createRequire(filename) }
  })
}

for (const version of ['2.3.2', '4.0.7']) {
  test(`picomatch ${version}: repeated wildcard extglobs are bounded`, () => {
    for (const { filename } of requirePackage('picomatch', version)) {
      // A separate process bounds the regression even if the patch is removed.
      const result = spawnSync(
        process.execPath,
        ['--input-type=commonjs', '-e', `
          const assert = require('node:assert/strict');
          const pm = require(${JSON.stringify(path.dirname(filename))});
          const patterns = ['+(*)', '*(*)', '+(*a)', '*(*a)', '+(*?)', '+(a*)', '+(+(*a))'];
          for (const pattern of patterns) {
            assert.equal(pm.isMatch(pattern, pattern), true, pattern + ' must be literalized');
            assert.equal(pm.isMatch('a'.repeat(48) + '!', pattern), false, pattern);
            assert.equal(pm.isMatch('a'.repeat(48) + '/', pattern), false, pattern);
          }
          assert.equal(pm.isMatch('src/index.ts', '**/*.ts'), true);
          assert.equal(pm.isMatch('foofoo', '+(foo|bar)'), true);
          assert.equal(pm.isMatch('aaa', '+(*(a))'), true);
          assert.equal(pm.isMatch('***', '+(\\\\*)'), true);
          assert.equal(pm.isMatch('+++', '+([+])'), true);
          assert.equal(pm.isMatch('aaa', '+(a)'), true);
          assert.equal(pm.isMatch('file.ts', '!(*.js)'), true);
          assert.equal(pm.isMatch('aaa', '+(*a)', { maxExtglobRecursion: false }), true);
        `],
        { encoding: 'utf8', timeout: 3000 },
      )
      assert.equal(result.error, undefined, result.error?.message)
      assert.equal(result.status, 0, result.stderr)
    }
  })
}

test('minimatch 11 preserves partial matching required by typedoc', () => {
  for (const { load } of requirePackage('minimatch', '11.')) {
    const { Minimatch } = load('minimatch')
    assert.equal(new Minimatch('src/**/*.ts').match('src', true), true)
    assert.equal(new Minimatch('src/**/*.ts').match('src/index.ts'), true)
    assert.equal(new Minimatch('src/**/*.ts').match('src/index.js'), false)
    assert.equal(new Minimatch('!**/node_modules/**').match('src/index.ts'), true)
  }
})

test('KaTeX 0.18 renders math and does not trust unsafe HTML commands', () => {
  for (const { load } of requirePackage('katex', '0.18.')) {
    const katex = load('katex')
    assert.match(katex.renderToString('c = \\pm\\sqrt{a^2 + b^2}'), /katex/)
    const html = katex.renderToString('\\href{javascript:alert(1)}{click}', {
      trust: false,
      strict: 'ignore',
    })
    assert.doesNotMatch(html, /href="javascript:/)
  }
})
