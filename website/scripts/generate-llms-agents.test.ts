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

test('buildLlmsTxt is an understanding-first explainer (no spin-up instructions)', async () => {
  const html = await fs.readFile(INDEX_PATH, 'utf8')
  const text = buildLlmsTxt(html)
  assert.ok(text.startsWith('# iii\n'))
  assert.ok(text.includes('> iii turns distributed'))
  assert.ok(text.includes('## Three primitives'))
  assert.ok(text.includes('## How iii compares'))
  assert.ok(text.includes('## Core pages'))
  assert.ok(text.includes('[llms.txt](https://iii.dev/llms.txt)'))
  assert.ok(text.includes('Homepage copy (extracted'))
  // Chat mode explains iii; it must NOT tell the reader to install / spin up iii.
  // Those action blocks live in AGENTS.md, which llms.txt points to as the build path.
  assert.ok(!text.includes('## Guardrails'))
  assert.ok(!text.includes('## Install / start'))
  assert.ok(!text.includes('npx skills add iii-hq/iii/skills'))
  assert.ok(!text.includes('install.iii.dev'))
  assert.ok(text.includes('[AGENTS.md](https://iii.dev/AGENTS.md)'))
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
  assert.ok(md.includes('## Guardrails'))
  assert.ok(md.includes('## Agent skills (after onboarding)'))
  assert.ok(md.includes('npx skills add iii-hq/iii/skills'))
  assert.ok(md.includes('Last updated:'))
})
