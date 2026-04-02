import {
  getApiTriggers,
  getCronTriggers,
  getQueueTriggers,
  getStateTriggers,
  getStreamTriggers,
  isApiTrigger,
  isCronTrigger,
  isQueueTrigger,
  isStateTrigger,
  isStreamTrigger,
} from '../src/guards'
import { cron, http, queue, state, stream } from '../src/triggers'

describe('guards', () => {
  describe('type guards', () => {
    it('isApiTrigger returns true for http trigger', () => {
      const t = http('GET', '/test')
      expect(isApiTrigger(t)).toBe(true)
    })

    it('isApiTrigger returns false for other trigger types', () => {
      expect(isApiTrigger(queue('x'))).toBe(false)
      expect(isApiTrigger(cron('0 0 * * *'))).toBe(false)
      expect(isApiTrigger(state())).toBe(false)
      expect(isApiTrigger(stream('s'))).toBe(false)
    })

    it('isQueueTrigger returns true for queue trigger', () => {
      const t = queue('tasks')
      expect(isQueueTrigger(t)).toBe(true)
    })

    it('isQueueTrigger returns false for other trigger types', () => {
      expect(isQueueTrigger(http('GET', '/x'))).toBe(false)
      expect(isQueueTrigger(cron('0 0 * * *'))).toBe(false)
      expect(isQueueTrigger(state())).toBe(false)
      expect(isQueueTrigger(stream('s'))).toBe(false)
    })

    it('isCronTrigger returns true for cron trigger', () => {
      const t = cron('0 * * * *')
      expect(isCronTrigger(t)).toBe(true)
    })

    it('isCronTrigger returns false for other trigger types', () => {
      expect(isCronTrigger(http('GET', '/x'))).toBe(false)
      expect(isCronTrigger(queue('x'))).toBe(false)
      expect(isCronTrigger(state())).toBe(false)
      expect(isCronTrigger(stream('s'))).toBe(false)
    })

    it('isStateTrigger returns true for state trigger', () => {
      const t = state()
      expect(isStateTrigger(t)).toBe(true)
    })

    it('isStateTrigger returns false for other trigger types', () => {
      expect(isStateTrigger(http('GET', '/x'))).toBe(false)
      expect(isStateTrigger(queue('x'))).toBe(false)
      expect(isStateTrigger(cron('0 0 * * *'))).toBe(false)
      expect(isStateTrigger(stream('s'))).toBe(false)
    })

    it('isStreamTrigger returns true for stream trigger', () => {
      const t = stream('events')
      expect(isStreamTrigger(t)).toBe(true)
    })

    it('isStreamTrigger returns false for other trigger types', () => {
      expect(isStreamTrigger(http('GET', '/x'))).toBe(false)
      expect(isStreamTrigger(queue('x'))).toBe(false)
      expect(isStreamTrigger(cron('0 0 * * *'))).toBe(false)
      expect(isStreamTrigger(state())).toBe(false)
    })
  })

  describe('filter helpers', () => {
    it('getApiTriggers filters api triggers from mixed array', () => {
      const step = {
        filePath: 'test.step.ts',
        config: {
          name: 'test',
          triggers: [http('GET', '/a'), queue('q'), http('POST', '/b'), state()],
        },
      }
      const result = getApiTriggers(step)
      expect(result).toHaveLength(2)
      expect(result[0].path).toBe('/a')
      expect(result[1].path).toBe('/b')
    })

    it('getQueueTriggers filters queue triggers from mixed array', () => {
      const step = {
        filePath: 'test.step.ts',
        config: {
          name: 'test',
          triggers: [queue('a'), http('GET', '/x'), queue('b'), cron('0 0 * * *')],
        },
      }
      const result = getQueueTriggers(step)
      expect(result).toHaveLength(2)
      expect(result[0].topic).toBe('a')
      expect(result[1].topic).toBe('b')
    })

    it('getCronTriggers filters cron triggers from mixed array', () => {
      const step = {
        filePath: 'test.step.ts',
        config: {
          name: 'test',
          triggers: [cron('0 * * * *'), queue('q'), cron('0 0 * * *')],
        },
      }
      const result = getCronTriggers(step)
      expect(result).toHaveLength(2)
      expect(result[0].expression).toBe('0 * * * *')
      expect(result[1].expression).toBe('0 0 * * *')
    })

    it('getStateTriggers filters state triggers from mixed array', () => {
      const step = {
        filePath: 'test.step.ts',
        config: {
          name: 'test',
          triggers: [state(), queue('q'), state({ keys: ['a'] }), http('GET', '/x')],
        },
      }
      const result = getStateTriggers(step)
      expect(result).toHaveLength(2)
      expect(result[0].type).toBe('state')
      expect(result[1].type).toBe('state')
    })

    it('getStreamTriggers filters stream triggers from mixed array', () => {
      const step = {
        filePath: 'test.step.ts',
        config: {
          name: 'test',
          triggers: [stream('a'), cron('0 0 * * *'), stream('b'), state()],
        },
      }
      const result = getStreamTriggers(step)
      expect(result).toHaveLength(2)
      expect(result[0].streamName).toBe('a')
      expect(result[1].streamName).toBe('b')
    })
  })
})
