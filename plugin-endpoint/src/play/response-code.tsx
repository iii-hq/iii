import { cn } from '@motiadev/ui'
import type { FC } from 'react'
import { getStatusMessage } from './reponse-code/utils'

type ResponseCodeProps = {
  statusCode: number
}

export const ResponseCode: FC<ResponseCodeProps> = ({ statusCode }) => {
  const statusMessage = getStatusMessage(statusCode)
  const isSuccess = statusCode > 0 && statusCode < 400
  const isWarning = statusCode >= 400 && statusCode < 500
  const isError = statusCode >= 500

  return (
    <div
      className={cn(
        'px-2 py-1 rounded-sm flex items-center gap-1',
        isWarning && 'dark:bg-[#EAB71F]/20 dark:text-[#EAB71F] bg-[#EAB71F] text-white',
        isError && 'dark:bg-[#F8367D]/20 dark:text-[#F8367D] bg-[#F8367D] text-white',
        isSuccess && 'dark:bg-accent-200 dark:text-primary bg-accent text-white',
      )}
    >
      <span className="font-bold font-mono">{statusCode}</span> {statusMessage}
    </div>
  )
}
