import { SequencePlayer } from '@/components/diagrams/SequencePlayer'
import { PageShell } from '@/components/PageShell'
import { C, CodeBlock, K, M, S } from '@/components/schematic/CodeBlock'
import { SEQ_LANES, SEQ_STEPS } from '@/content/example'

/**
 * A14 — a deep-dive page (`#/example` route). Use one per distinct
 * consumer/scenario worth its own full page: a focused sequence plus the code
 * a reader would actually write. Cross-link siblings via PageShell `related`.
 */
export function ExamplePage() {
  return (
    <PageShell
      eyebrow="deep dive"
      title="a worked example, end to end"
      description="a deep-dive page lives at its own #/slug route. give one to each scenario that deserves the full walkthrough — a sequence, the real code, and the contracts behind it."
    >
      <SequencePlayer
        title="the example flow"
        lanes={SEQ_LANES}
        steps={SEQ_STEPS}
      />

      <CodeBlock title="what the caller writes">
        <K>await</K> client.<K>request</K>(<M>{'{'}</M>
        {'\n  '}input: <S>"do the thing"</S>,
        {'\n  '}on: <M>{'{'}</M> done: (r) <M>{'=>'}</M> render(r) <M>{'}'}</M>,{' '}
        <C>{'// bound once, fires live'}</C>
        {'\n'}
        <M>{'})'}</M>
      </CodeBlock>
    </PageShell>
  )
}
