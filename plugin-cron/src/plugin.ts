import type { MotiaPlugin, MotiaPluginContext } from '@motiadev/core'

export default function plugin(_motia: MotiaPluginContext): MotiaPlugin {
  return {
    workbench: [
      {
        packageName: '@motiadev/cron-plugin',
        cssImports: ['@motiadev/cron-plugin/dist/styles.css'],
        label: 'Cron Jobs',
        position: 'top',
        componentName: 'CronJobsPage',
        labelIcon: 'clock',
      },
    ],
  }
}
