import { AlertCircle, CheckCircle2, Copy } from 'lucide-react'
import type { VisualizationSpan } from '@/lib/traceTransform'
import { useCopyToClipboard } from '@/lib/traceUtils'

interface SpanErrorsTabProps {
  span: VisualizationSpan
}

export function SpanErrorsTab({ span }: SpanErrorsTabProps) {
  const { copiedKey, copy } = useCopyToClipboard()
  const exceptionEvent = span.events?.find(
    (e) => e.name === 'exception' || e.name?.startsWith('exception'),
  )
  const hasError = span.status === 'error' || !!exceptionEvent
  const eventAttrs = exceptionEvent?.attributes ?? {}

  const errorMessage = span.attributes?.['error.message'] as string | undefined
  const errorType = span.attributes?.['error.type'] as string | undefined
  const errorStack = span.attributes?.['error.stack'] as string | undefined
  const exceptionMessage = (span.attributes?.['exception.message'] ??
    eventAttrs['exception.message']) as string | undefined
  const exceptionType = (span.attributes?.['exception.type'] ?? eventAttrs['exception.type']) as
    | string
    | undefined
  const exceptionStacktrace = (span.attributes?.['exception.stacktrace'] ??
    eventAttrs['exception.stacktrace']) as string | undefined

  const displayMessage = errorMessage || exceptionMessage
  const displayType = errorType || exceptionType
  const displayStack = errorStack || exceptionStacktrace

  const copyStackTrace = () => {
    if (displayStack) {
      copy('stackTrace', displayStack)
    }
  }

  if (!hasError) {
    return (
      <div className="p-8 text-center">
        <div className="w-12 h-12 mb-3 mx-auto rounded-full bg-[#22C55E]/10 border border-[#22C55E]/20 flex items-center justify-center">
          <CheckCircle2 className="w-6 h-6 text-[#22C55E]" />
        </div>
        <p className="text-sm font-medium text-[#F4F4F4]">No errors</p>
        <p className="text-[11px] text-gray-500 mt-1">This span completed successfully</p>
      </div>
    )
  }

  return (
    <div className="p-5 space-y-4">
      {/* Error banner */}
      <div className="bg-[#EF4444]/5 border border-[#EF4444]/15 rounded-lg p-4">
        <div className="flex items-start gap-3">
          <div className="w-8 h-8 rounded-lg bg-[#EF4444]/10 border border-[#EF4444]/20 flex items-center justify-center flex-shrink-0">
            <AlertCircle className="w-4 h-4 text-[#EF4444]" />
          </div>
          <div className="flex-1 min-w-0">
            {displayType && (
              <div className="text-sm font-semibold text-[#EF4444] font-mono mb-1">
                {displayType}
              </div>
            )}
            {displayMessage && (
              <div className="text-sm text-gray-300 break-words leading-relaxed">
                {displayMessage}
              </div>
            )}
            {!displayType && !displayMessage && (
              <div className="text-sm text-gray-400">Error status with no additional details</div>
            )}
          </div>
        </div>
      </div>

      {/* Stack trace */}
      {displayStack && (
        <div>
          <div className="flex items-center justify-between mb-2">
            <span className="text-[10px] font-semibold text-gray-500 uppercase tracking-wider">
              Stack Trace
            </span>
            <button
              type="button"
              onClick={copyStackTrace}
              className="flex items-center gap-1.5 px-2 py-1 text-[10px] text-gray-500 hover:text-gray-300 hover:bg-[#1D1D1D] rounded transition-colors"
            >
              {copiedKey === 'stackTrace' ? (
                <span className="text-[#22C55E]">Copied</span>
              ) : (
                <>
                  <Copy className="w-2.5 h-2.5" />
                  <span>Copy</span>
                </>
              )}
            </button>
          </div>
          <div className="bg-[#141414] rounded-lg border border-[#1D1D1D] overflow-hidden">
            <pre className="p-4 text-[11px] font-mono text-gray-300 whitespace-pre-wrap break-words overflow-x-auto leading-[1.6] max-h-[400px] overflow-y-auto">
              {displayStack.split('\n').map((line, i) => {
                const isFrameLine =
                  /^\s+at\s/.test(line) || /\.(ts|js|tsx|jsx|py|go|rs|java)[:(\s]/.test(line)
                const hasLineNumber = /:\d+[:\d]*/.test(line)
                return (
                  <div
                    key={`${i}:${line}`}
                    className={`${
                      isFrameLine
                        ? hasLineNumber
                          ? 'text-gray-300 hover:bg-[#1A1A1A]'
                          : 'text-gray-400'
                        : 'text-[#EF4444] font-medium'
                    } px-1 -mx-1 rounded`}
                  >
                    {line}
                  </div>
                )
              })}
            </pre>
          </div>
        </div>
      )}

      {!displayMessage && !displayType && !displayStack && (
        <div className="bg-[#141414] rounded-lg border border-[#1D1D1D] p-4 text-center">
          <p className="text-sm text-gray-400">No additional error details</p>
          <p className="text-[11px] text-gray-600 mt-1">
            The span is marked as error but no error attributes were recorded
          </p>
        </div>
      )}
    </div>
  )
}
