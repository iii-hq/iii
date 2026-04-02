import { type Handlers, logger, type StateTriggerInput, type StepConfig } from 'motia'
import type { ParallelMergeResult } from './parallel-merge.types'

export const config = {
  name: 'parallel-merge-join',
  description: 'Merges the results of parallel steps',
  triggers: [
    {
      type: 'state',
      condition: (input: StateTriggerInput<ParallelMergeResult>) => {
        return (
          input.group_id === 'users' &&
          !!input.new_value &&
          input.new_value.totalSteps === input.new_value.completedSteps
        )
      },
    },
  ],
  flows: ['parallel-merge'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async (request) => {
  logger.info('[join-complete] Handling Join Complete', { request })

  const endTime = Date.now()
  const result = request.new_value as ParallelMergeResult
  const traceId = request.item_id

  await new Promise((resolve) => setTimeout(resolve, 500))

  console.log('░█████████     ░███    ░█████████     ░███    ░██         ░██         ░██████████ ░██            ')
  console.log('░██     ░██   ░██░██   ░██     ░██   ░██░██   ░██         ░██         ░██         ░██            ')
  console.log('░██     ░██  ░██  ░██  ░██     ░██  ░██  ░██  ░██         ░██         ░██         ░██            ')
  console.log('░█████████  ░█████████ ░█████████  ░█████████ ░██         ░██         ░█████████  ░██            ')
  console.log('░██         ░██    ░██ ░██   ░██   ░██    ░██ ░██         ░██         ░██         ░██            ')
  console.log('░██         ░██    ░██ ░██    ░██  ░██    ░██ ░██         ░██         ░██         ░██            ')
  console.log('░██         ░██    ░██ ░██     ░██ ░██    ░██ ░██████████ ░██████████ ░██████████ ░██████████    ')
  console.log('                                                                                                 ')
  console.log('                                                                                                 ')
  console.log('                                                                                                 ')
  console.log('                                 ░███     ░███ ░██████████ ░█████████    ░██████  ░██████████    ')
  console.log('                                 ░████   ░████ ░██         ░██     ░██  ░██   ░██ ░██            ')
  console.log('                                 ░██░██ ░██░██ ░██         ░██     ░██ ░██        ░██            ')
  console.log('                                 ░██ ░████ ░██ ░█████████  ░█████████  ░██  █████ ░█████████     ')
  console.log('                                 ░██  ░██  ░██ ░██         ░██   ░██   ░██     ██ ░██            ')
  console.log('                                 ░██       ░██ ░██         ░██    ░██   ░██  ░███ ░██            ')
  console.log('                                 ░██       ░██ ░██████████ ░██     ░██   ░█████░█ ░██████████  ')
  console.log('                                                                                                 ')
  console.log('                                                                                                 ')
  console.log(`Total # of steps: ${result.totalSteps}`)
  console.log(`Total time taken: ${endTime - result.startedAt}ms`)
  console.log(`Trace ID: ${traceId}`)
}
