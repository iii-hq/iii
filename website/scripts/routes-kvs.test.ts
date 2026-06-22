import assert from 'node:assert/strict'
import { test } from 'node:test'
import { desiredRoutes, diff } from './routes-kvs'

test('desiredRoutes maps page files to pretty keys and excludes index.html', () => {
  const result = desiredRoutes(['index.html', 'manifesto.html', 'privacy-policy.html'])
  assert.deepEqual(result, [
    { Key: '/manifesto', Value: '/manifesto.html' },
    { Key: '/privacy-policy', Value: '/privacy-policy.html' },
  ])
})

test('desiredRoutes ignores non-html files and sorts by key', () => {
  const result = desiredRoutes(['pricing.html', 'favicon.svg', 'about.html', 'sitemap.xml'])
  assert.deepEqual(result, [
    { Key: '/about', Value: '/about.html' },
    { Key: '/pricing', Value: '/pricing.html' },
  ])
})

test('diff: a new page becomes a Put, nothing deleted', () => {
  const desired = [
    { Key: '/manifesto', Value: '/manifesto.html' },
    { Key: '/pricing', Value: '/pricing.html' },
  ]
  const current = [{ Key: '/manifesto', Value: '/manifesto.html' }]
  assert.deepEqual(diff(desired, current), {
    Puts: [{ Key: '/pricing', Value: '/pricing.html' }],
    Deletes: [],
  })
})

test('diff: a removed page becomes a Delete', () => {
  const desired = [{ Key: '/manifesto', Value: '/manifesto.html' }]
  const current = [
    { Key: '/manifesto', Value: '/manifesto.html' },
    { Key: '/old', Value: '/old.html' },
  ]
  assert.deepEqual(diff(desired, current), {
    Puts: [],
    Deletes: [{ Key: '/old' }],
  })
})

test('diff: a changed value becomes a Put', () => {
  const desired = [{ Key: '/legal', Value: '/privacy-policy.html' }]
  const current = [{ Key: '/legal', Value: '/legal.html' }]
  assert.deepEqual(diff(desired, current), {
    Puts: [{ Key: '/legal', Value: '/privacy-policy.html' }],
    Deletes: [],
  })
})

test('diff: identical desired and current is a no-op', () => {
  const entries = [
    { Key: '/manifesto', Value: '/manifesto.html' },
    { Key: '/privacy-policy', Value: '/privacy-policy.html' },
  ]
  assert.deepEqual(diff(entries, [...entries]), { Puts: [], Deletes: [] })
})
