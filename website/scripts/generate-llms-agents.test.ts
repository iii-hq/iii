import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  buildAgentsMd,
  buildHomepageExtractFromHtml,
  buildLlmsTxt,
  overviewBodyWithoutLeadingH1,
} from './generate-llms-agents'

const INDEX_PATH = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../index.html')
const APPENDIX_PATH = path.resolve(path.dirname(fileURLToPath(import.meta.url)), 'agents-appendix.md')

test('overviewBodyWithoutLeadingH1 drops duplicate H1 for llms.txt', () => {
  const body = overviewBodyWithoutLeadingH1()
  assert.ok(!body.startsWith('# '))
  assert.ok(body.includes('Three primitives'))
})

test('buildLlmsTxt follows llms.txt-style shape (H1, blockquote, sections)', async () => {
  const html = await fs.readFile(INDEX_PATH, 'utf8')
  const text = buildLlmsTxt(html)
  assert.ok(text.startsWith('# iii\n'))
  assert.ok(text.includes('> iii turns distributed'))
  assert.ok(text.includes('## Core pages'))
  assert.ok(text.includes('[llms.txt](https://iii.dev/llms.txt)'))
  assert.ok(text.includes('Homepage copy (extracted'))
})

test('buildHomepageExtractFromHtml pulls hero prose but not hello code fences', async () => {
  const html = await fs.readFile(INDEX_PATH, 'utf8')
  const text = buildHomepageExtractFromHtml(html)
  assert.ok(text.includes('unreasonably simple'))
  assert.ok(text.includes('Node.js Worker'))
  assert.ok(!text.includes('registerWorker'))
  assert.ok(!text.includes('```'))
})

test('buildAgentsMd includes agents.md framing and appendix', async () => {
  const html = await fs.readFile(INDEX_PATH, 'utf8')
  const appendix = await fs.readFile(APPENDIX_PATH, 'utf8')
  const md = buildAgentsMd(html, appendix)
  assert.ok(md.startsWith('# iii for AI Agents'))
  assert.ok(md.includes('agents.md'))
  assert.ok(md.includes('## Overview and comparisons'))
  assert.ok(md.includes('## Primitives (wire-level)'))
  assert.ok(md.includes('Last updated:'))
})
