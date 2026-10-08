import { Reveal } from '@/components/site/reveal'
import { tokenize } from '@/lib/highlight'
import { demo } from './content'
import styles from './live-demo.module.css'
import { LiveDemoClient } from './live-demo-client'
import { demoCode } from './live-demo-code'
import { Section } from './section'

/** Server shell: highlights the demo code once, then hands the tokens to the interactive demo. */
export async function LiveDemo() {
  const code = await tokenize(demoCode.lines.join('\n'), demoCode.lang)
  return (
    // biome-ignore lint/correctness/useUniqueElementIds: One stable anchor per section on this page.
    <Section id="demo" className={styles.section} eyebrow={demo.eyebrow} title={demo.title} lede={demo.subtitle}>
      <Reveal delay={0.1}>
        <LiveDemoClient code={code} />
      </Reveal>
    </Section>
  )
}
