import type { TriggerData } from '../../../api/flows/types'
import { TriggerIcon } from './trigger-icon'

export function TriggerList({ triggers }: { triggers: TriggerData[] }) {
  if (triggers.length <= 1) return null

  return (
    <div className="space-y-1.5">
      <div className="text-[10px] font-semibold text-[#9CA3AF] uppercase tracking-wider">
        Triggers ({triggers.length})
      </div>
      <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-1.5 items-center text-[10px]">
        {triggers.map((trigger, i) => (
          <div
            key={`${trigger.type}-${trigger.topic ?? trigger.path ?? trigger.cronExpression ?? i}`}
            className="contents"
          >
            <span className="px-1.5 py-0.5 rounded bg-[#1D1D1D] text-[#9CA3AF] font-mono flex items-center gap-1">
              <TriggerIcon type={trigger.type} />
              {trigger.type}
            </span>
            <span className="text-[#9CA3AF] font-mono truncate">
              {trigger.type === 'event' && trigger.topic}
              {trigger.type === 'http' &&
                trigger.method &&
                trigger.path &&
                `${trigger.method} ${trigger.path}`}
              {trigger.type === 'cron' && trigger.cronExpression}
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}
