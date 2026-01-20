import type { MotiaPlugin, MotiaPluginContext } from '@motiadev/core'

export default function plugin(_motia: MotiaPluginContext): MotiaPlugin {
  return {
    workbench: [
      {
        packageName: '@motiadev/ws-plugin',
        cssImports: ['@motiadev/ws-plugin/dist/styles.css'],
        label: 'WebSockets',
        position: 'bottom',
        componentName: 'WebSocketsPage',
        labelIcon: 'radio',
      },
    ],
  }
}
