import type { MotiaPlugin, MotiaPluginContext } from '@motiadev/core'

export default function plugin(_motia: MotiaPluginContext): MotiaPlugin {
  return {
    workbench: [
      {
        packageName: '@motiadev/plugin-logs',
        label: 'Logs',
        position: 'bottom',
        componentName: 'LogsPage',
        labelIcon: 'logs',
      },
    ],
  }
}
