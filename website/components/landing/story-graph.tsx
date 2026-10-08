import {
  BotIcon,
  DatabaseIcon,
  FolderIcon,
  GitPullRequestIcon,
  GlobeIcon,
  SearchIcon,
  TerminalIcon,
  WorkflowIcon,
} from 'lucide-react'

import { cn } from '@/lib/utils'
import styles from './story.module.css'
import {
  ENGINE_POINT,
  STORY_BEATS,
  STORY_NODES,
  type StoryNodeId,
  type StoryStage,
  storyNodeVisible,
} from './story-model'

const ICONS = {
  http: GlobeIcon,
  api: TerminalIcon,
  database: DatabaseIcon,
  directory: SearchIcon,
  storage: FolderIcon,
  router: WorkflowIcon,
  anthropic: BotIcon,
  openai: BotIcon,
  harness: BotIcon,
  github: GitPullRequestIcon,
}
const NODE_BY_ID = Object.fromEntries(STORY_NODES.map((node) => [node.id, node])) as Record<
  StoryNodeId,
  (typeof STORY_NODES)[number]
>

/** Route around intervening nodes so a connection never implies a call through another worker. */
function nodePoint(id: StoryNodeId, stage: StoryStage) {
  const node = NODE_BY_ID[id]
  return stage === 'compose' ? { x: node.x, y: id === 'api' ? 12 : 26 } : node
}

function engineRoute(id: StoryNodeId, stage: StoryStage) {
  const node = nodePoint(id, stage)
  if (stage === 'compose') return [node, ENGINE_POINT]
  if (id === 'api') return [node, { x: 34, y: node.y }, { x: 34, y: 39 }, ENGINE_POINT]
  if (id === 'harness') return [node, { x: 66, y: node.y }, { x: 66, y: 53 }, ENGINE_POINT]
  return [node, ENGINE_POINT]
}

function pathFromPoints(points: { x: number; y: number }[]) {
  return points.map((point, i) => `${i === 0 ? 'M' : 'L'} ${point.x} ${point.y}`).join(' ')
}

/** The topology persists between chapters; only joining workers and the active route change. */
export function StoryGraph({ stage, beat, running }: { stage: StoryStage; beat: number; running: boolean }) {
  const current = STORY_BEATS[stage][beat]
  const visible = STORY_NODES.filter((node) => storyNodeVisible(node, stage, beat))
  const active = new Set(current.active)
  return (
    <figure className={styles.graph} data-stage={stage} data-beat={beat}>
      <figcaption className={styles.graphHeading}>
        <span>One connected system</span>
        <span>{visible.length} workers shown</span>
      </figcaption>
      <div className={styles.canvas} aria-hidden="true" data-running={running}>
        <svg viewBox="0 0 100 100" preserveAspectRatio="none" className={styles.edges} aria-hidden="true">
          {visible.map((node) => (
            <path
              key={node.id}
              d={pathFromPoints(engineRoute(node.id, stage))}
              className={cn(styles.edge, active.has(node.id) && styles.edgeActive)}
              vectorEffect="non-scaling-stroke"
            />
          ))}
          {current.route ? (
            <GraphRoute key={`${stage}-${beat}`} stage={stage} route={current.route} running={running} />
          ) : null}
        </svg>
        <div className={styles.engine} style={{ left: `${ENGINE_POINT.x}%`, top: `${ENGINE_POINT.y}%` }}>
          <span className={styles.engineWord}>iii</span>
          <span>engine</span>
        </div>
        {visible.map((node) => {
          const Icon = ICONS[node.id]
          const point = nodePoint(node.id, stage)
          return (
            <div
              key={node.id}
              className={cn(styles.node, active.has(node.id) && styles.nodeActive)}
              style={{ left: `${point.x}%`, top: `${point.y}%` }}
            >
              <Icon aria-hidden />
              <span className={styles.nodeLabel}>{node.name}</span>
              <span className={styles.nodeDetail}>{node.detail}</span>
            </div>
          )
        })}
        {visible.length === 0 && stage !== 'compose' ? (
          <p className={styles.graphEmpty}>Your system starts here.</p>
        ) : null}
        {stage === 'compose' ? <ComposeRequest beat={beat} /> : null}
      </div>
      <p className={styles.graphCaption}>{current.caption}</p>
      {stage === 'compose' ? (
        <p className="sr-only">
          Example: POST /orders calls orders::create through iii, then the API calls database::execute through iii.
          {beat >= 6
            ? ' Order saved. The caller receives 201 Created.'
            : ' Waiting for the example request to complete.'}
        </p>
      ) : null}
      <ul className="sr-only" aria-label="Workers connected to iii">
        {visible.map((node) => (
          <li key={node.id}>
            {node.name}: {node.detail}
          </li>
        ))}
      </ul>
    </figure>
  )
}

function GraphRoute({ stage, route, running }: { stage: StoryStage; route: StoryNodeId[]; running: boolean }) {
  const points = engineRoute(route[0], stage)
  if (route[1]) points.push(...engineRoute(route[1], stage).reverse().slice(1))
  const path = pathFromPoints(points)
  return (
    <g>
      <path d={path} className={styles.route} vectorEffect="non-scaling-stroke" />
      {running ? (
        <circle r="0.65" className={styles.packet}>
          <animateMotion dur="1.05s" repeatCount="1" fill="freeze" path={path} />
        </circle>
      ) : null}
    </g>
  )
}

/** A concrete outcome gives the connection diagram a purpose, even with motion disabled. */
function ComposeRequest({ beat }: { beat: number }) {
  return (
    <div className={styles.composeRequest} data-complete={beat >= 6}>
      <div className={styles.requestHeading}>
        <code>POST /orders</code>
        <span>{beat >= 6 ? '201 Created' : beat >= 4 ? 'Processing' : 'Example request'}</span>
      </div>
      <ol className={styles.requestCalls}>
        <li data-active={beat >= 4}>
          <span>http</span>
          <span aria-hidden>→</span>
          <b>iii</b>
          <span aria-hidden>→</span>
          <code>orders::create</code>
        </li>
        <li data-active={beat >= 5}>
          <span>api</span>
          <span aria-hidden>→</span>
          <b>iii</b>
          <span aria-hidden>→</span>
          <code>database::execute</code>
        </li>
      </ol>
      <p>{beat >= 6 ? 'Order saved. Response returned to the caller.' : 'One request across three workers.'}</p>
    </div>
  )
}
