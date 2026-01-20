import type { MotiaPlugin, MotiaPluginContext, Step, StreamConfig } from '@motiadev/core'
import { api } from './api'
import { StreamsRegistryAdapter } from './streams/streams-registry'
import type { StreamInfo } from './types/stream'

const STREAM_NAME = '__motia.streams-registry'

type StreamRegistryInfo = StreamInfo & { id: string }

const isStreamStep = (step: Step): step is Step<StreamConfig> => {
  return step.type === 'stream'
}

const mapStreamToInfo = (step: Step<StreamConfig>): StreamRegistryInfo => ({
  id: step.config.name,
  name: step.config.name,
  hidden: step.hidden || false,
  itemCount: 0,
  groupCount: 0,
  lastUpdated: Date.now(),
  filePath: step.filePath,
})

export default function plugin(motia: MotiaPluginContext): MotiaPlugin {
  const { lockedData } = motia

  api(motia)

  const registryAdapter = new StreamsRegistryAdapter(lockedData)

  const stream = lockedData.createStream({
    filePath: `${STREAM_NAME}.ts`,
    hidden: true,
    config: {
      name: STREAM_NAME,
      baseConfig: { storageType: 'custom', factory: () => registryAdapter },
      schema: null as never,
    },
  })()

  const handleStreamCreated = (step: Step) => {
    if (isStreamStep(step)) {
      stream.set('default', step.config.name, mapStreamToInfo(step))
    }
  }

  const handleStreamUpdated = (step: Step) => {
    if (isStreamStep(step)) {
      stream.set('default', step.config.name, mapStreamToInfo(step))
    }
  }

  const handleStreamRemoved = (step: Step) => {
    if (isStreamStep(step)) {
      stream.delete('default', step.config.name)
    }
  }

  lockedData.onStep('step-created', handleStreamCreated)
  lockedData.onStep('step-updated', handleStreamUpdated)
  lockedData.onStep('step-removed', handleStreamRemoved)

  setTimeout(() => {
    const existingStreams = lockedData.listStreams()
    for (const existingStream of existingStreams) {
      const streamName = existingStream.config.name
      if (existingStream.hidden || streamName.startsWith('__motia.')) {
        continue
      }

      stream.set('default', streamName, {
        id: streamName,
        name: streamName,
        hidden: existingStream.hidden || false,
        itemCount: 0,
        groupCount: 0,
        lastUpdated: Date.now(),
        filePath: existingStream.filePath,
      })
    }
  }, 100)

  return {
    workbench: [
      {
        packageName: '@motiadev/plugin-streams',
        cssImports: ['@motiadev/plugin-streams/dist/styles.css'],
        label: 'Streams',
        position: 'top',
        componentName: 'StreamsPage',
        labelIcon: 'database',
      },
    ],
  }
}
