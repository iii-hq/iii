import { Funnel } from '@lib/components/diagrams/Funnel'
import { Section } from '@lib/components/Section'
import { SpecRow, SpecSheet } from '@lib/components/SpecSheet'
import { CONFIG_PATHS, CONFIG_REJECT, CONFIG_RULES, CONFIG_TARGET } from '../content/config'

/**
 * A8 — three config sources converge on one started process; the fourth path
 * (a full config body in yaml) is the one this design rejects.
 */
export function ConfigSection() {
  return (
    <Section
      id="config"
      index="06"
      eyebrow="configuration"
      title="the worker is born configured."
      lede="the daemon fetches the base by name, merges the file's sparse override, and hands the process its final config at start. no boot-query-restart dance, no second source of truth."
    >
      <Funnel
        title="defaults + base + override → one process"
        paths={CONFIG_PATHS}
        target={CONFIG_TARGET}
        reject={CONFIG_REJECT}
      />

      <div className="mt-6">
        <SpecSheet title="the rules" meta="config_name · config_override">
          {CONFIG_RULES.map((rule) => (
            <SpecRow key={rule.name} name={rule.name}>
              {rule.desc}
            </SpecRow>
          ))}
        </SpecSheet>
      </div>
    </Section>
  )
}
