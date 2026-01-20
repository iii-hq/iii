import type { MotiaPlugin, MotiaPluginContext } from '@motiadev/core'
import { api } from './api'

export default function plugin(motia: MotiaPluginContext): MotiaPlugin {
  api(motia)

  return {
    workbench: [
      {
        packageName: '@motiadev/plugin-observability',
        cssImports: ['@motiadev/plugin-observability/dist/styles.css'],
        label: 'Tracing',
        position: 'bottom',
        componentName: 'ObservabilityPage',
        labelIcon: 'gantt-chart',
      },
    ],
  }
}
