import { LevelDot, Sidebar } from '@motiadev/ui'
import { X } from 'lucide-react'
import type React from 'react'
import { useMemo, useState } from 'react'
import ReactJson from 'react18-json-view'
import 'react18-json-view/src/dark.css'
import 'react18-json-view/src/style.css'
import type { Log } from '../types/log'
import { formatTimestamp } from '../utils/format-timestamp'

type Props = {
  log?: Log
  onClose: () => void
}

const defaultProps = ['id', 'msg', 'time', 'level', 'step', 'flows', 'traceId']

export const LogDetail: React.FC<Props> = ({ log, onClose }) => {
  const [hasOtherProps, setHasOtherProps] = useState(false)

  const otherPropsObject = useMemo(() => {
    if (!log) {
      return null
    }

    const otherProps = Object.keys(log ?? {}).filter((key) => !defaultProps.includes(key))
    setHasOtherProps(otherProps.length > 0)

    return otherProps.reduce(
      (acc, key) => {
        acc[key] = log[key]
        return acc
      },
      {} as Record<string, unknown>,
    )
  }, [log])

  if (!log) {
    return null
  }

  return (
    <Sidebar
      onClose={onClose}
      title="Logs Details"
      subtitle="Details including custom properties"
      actions={[{ icon: <X />, onClick: onClose, label: 'Close' }]}
      details={[
        {
          label: 'Level',
          value: (
            <div className="flex items-center gap-2">
              <LevelDot level={log.level} />
              <div className="capitalize">{log.level}</div>
            </div>
          ),
        },
        { label: 'Time', value: formatTimestamp(log.time) },
        { label: 'Step', value: log.step },
        { label: 'Flows', value: log.flows.join(', ') },
        { label: 'Trace ID', value: log.traceId },
      ]}
    >
      {hasOtherProps && <ReactJson src={otherPropsObject} theme="default" enableClipboard />}
    </Sidebar>
  )
}
