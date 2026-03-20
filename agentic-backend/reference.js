/**
 * Pattern: Agentic Backend Infrastructure
 * Comparable to: LangGraph, CrewAI, AutoGen, Letta
 *
 * Demonstrates a multi-agent research pipeline where specialized agents
 * collaborate through named queues and shared state. Each agent processes a
 * task, writes its findings to state, and hands off to the next agent via
 * a named queue. Fan-out (completion broadcast) uses pubsub.
 *
 * How-to references:
 *   - Functions & Triggers: https://iii.dev/docs/how-to/use-functions-and-triggers
 *   - State management:    https://iii.dev/docs/how-to/manage-state
 *   - State reactions:     https://iii.dev/docs/how-to/react-to-state-changes
 *   - Queues:              https://iii.dev/docs/how-to/use-queues
 *   - Conditions:          https://iii.dev/docs/how-to/use-trigger-conditions
 */

import { registerWorker, Logger, TriggerAction } from 'iii-sdk'

const iii = registerWorker(process.env.III_ENGINE_URL || 'ws://localhost:49134', {
  workerName: 'agentic-backend',
})

// ---------------------------------------------------------------------------
// Agent 1 — Researcher: gathers raw information on a topic
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'agents::researcher' }, async (data) => {
  const logger = new Logger()
  logger.info('Researcher agent working', { topic: data.topic })

  const findings = {
    topic: data.topic,
    sources: ['arxiv', 'wikipedia', 'internal-kb'],
    summary: `Key findings on ${data.topic}: ...`,
    confidence: 0.82,
  }

  // Store findings in shared state so other agents can read them
  await iii.trigger({
    function_id: 'state::set',
    payload: {
      scope: 'research-tasks',
      key: data.task_id,
      value: {
        _key: data.task_id,
        task_id: data.task_id,
        topic: data.topic,
        phase: 'researched',
        findings,
      },
    },
  })

  // Hand off to the critic agent via named queue
  iii.trigger({
    function_id: 'agents::critic',
    payload: { task_id: data.task_id },
    action: TriggerAction.Enqueue({ queue: 'agent-tasks' }),
  })

  return findings
})

// ---------------------------------------------------------------------------
// Agent 2 — Critic: reviews and scores the researcher's output
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'agents::critic' }, async (data) => {
  const logger = new Logger()

  const task = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'research-tasks', key: data.task_id },
  })

  logger.info('Critic reviewing findings', { confidence: task.findings.confidence })

  const review = {
    score: task.findings.confidence > 0.7 ? 'pass' : 'needs-revision',
    feedback: 'Findings are well-sourced and relevant.',
  }

  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'research-tasks',
      key: data.task_id,
      ops: [
        { type: 'set', path: 'phase', value: 'reviewed' },
        { type: 'set', path: 'review', value: review },
      ],
    },
  })

  // Only hand off to the synthesizer if approved
  const approved = await iii.trigger({
    function_id: 'agents::is-approved',
    payload: { task_id: data.task_id },
  })

  if (approved) {
    iii.trigger({
      function_id: 'agents::synthesizer',
      payload: { task_id: data.task_id },
      action: TriggerAction.Enqueue({ queue: 'agent-tasks' }),
    })
  }

  return review
})

// ---------------------------------------------------------------------------
// Agent 3 — Synthesizer: produces a final report from reviewed findings
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'agents::synthesizer' }, async (data) => {
  const logger = new Logger()

  const task = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'research-tasks', key: data.task_id },
  })

  logger.info('Synthesizer creating report', { task_id: data.task_id })

  const report = {
    title: `Report: ${task.topic}`,
    body: `Based on ${task.findings.sources.length} sources...`,
    review_score: task.review.score,
    generated_at: new Date().toISOString(),
  }

  await iii.trigger({
    function_id: 'state::update',
    payload: {
      scope: 'research-tasks',
      key: data.task_id,
      ops: [
        { type: 'set', path: 'phase', value: 'complete' },
        { type: 'set', path: 'report', value: report },
      ],
    },
  })

  // Broadcast completion for any listening services
  iii.trigger({
    function_id: 'publish',
    payload: { topic: 'research.complete', data: { task_id: data.task_id, report } },
    action: TriggerAction.Void(),
  })

  return report
})

// ---------------------------------------------------------------------------
// Condition: only synthesize if the critic passed the findings
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'agents::is-approved' }, async (data) => {
  const task = await iii.trigger({
    function_id: 'state::get',
    payload: { scope: 'research-tasks', key: data.task_id },
  })
  return task?.review?.score === 'pass'
})

// ---------------------------------------------------------------------------
// HTTP trigger — kick off a research task
// ---------------------------------------------------------------------------
iii.registerFunction({ id: 'agents::start-research' }, async (data) => {
  const task_id = `task-${Date.now()}`
  iii.trigger({
    function_id: 'agents::researcher',
    payload: { task_id, topic: data.topic },
    action: TriggerAction.Enqueue({ queue: 'agent-tasks' }),
  })
  return { task_id, status: 'queued' }
})

iii.registerTrigger({
  type: 'http',
  function_id: 'agents::start-research',
  config: { api_path: '/agents/research', http_method: 'POST' },
})
