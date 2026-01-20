import { Fragment, memo, useMemo } from 'react'
import { CopyButton } from '../../../components/ui/copy-button'
import { Popover, PopoverContent, PopoverTrigger } from '../../../components/ui/popover'
import { Argument } from './argument'
import { ClosingParenthesis } from './closing-parenthesis'
import { FunctionCallContent } from './function-call-content'

type Props = {
  topLevelClassName?: string
  objectName?: string
  functionName: string
  args: Array<string | object | false | undefined>
  callsQuantity?: number
}

const GRID_LAYOUT_STYLE = { gridTemplateColumns: 'max-content 1fr max-content' }

export const FunctionCall: React.FC<Props> = memo(
  ({ topLevelClassName, objectName, functionName, args, callsQuantity }) => {
    const filteredArgs = useMemo(() => args.filter((arg) => arg !== undefined), [args])

    const argsGridStyle = useMemo(
      () => ({ gridTemplateColumns: `repeat(${filteredArgs.length * 2}, minmax(0, auto))` }),
      [filteredArgs.length],
    )

    const functionCallText = useMemo(() => {
      const prefix = [topLevelClassName, objectName].filter(Boolean).join('.')
      const fullPrefix = prefix ? `${prefix}.` : ''
      const argsString = filteredArgs
        .map((arg) => {
          if (typeof arg === 'object' && arg !== null) {
            return JSON.stringify(arg, null, 2)
          }
          if (arg === null) {
            return 'null'
          }
          if (typeof arg === 'string') {
            return `'${arg}'`
          }
          return String(arg)
        })
        .join(', ')
      return `${fullPrefix}${functionName}(${argsString})`
    }, [topLevelClassName, objectName, functionName, filteredArgs])

    return (
      <div className="grid overflow-hidden items-center" style={GRID_LAYOUT_STYLE}>
        <Popover>
          <PopoverTrigger asChild>
            <FunctionCallContent
              className="cursor-pointer"
              functionName={functionName}
              topLevelClassName={topLevelClassName}
              objectName={objectName}
            />
          </PopoverTrigger>
          <PopoverContent
            className="grid grid-rows-auto backdrop-blur-md w-auto max-w-2xl max-h-[50vh] overflow-y-auto"
            hideWhenDetached
          >
            <div className="flex items-center justify-between gap-2">
              <FunctionCallContent
                functionName={functionName}
                topLevelClassName={topLevelClassName}
                objectName={objectName}
              />
              <CopyButton textToCopy={functionCallText} />
            </div>
            {filteredArgs.map((arg, index) => (
              <div key={index} className="grid grid-cols-[auto_1fr] items-center pl-4">
                <Argument arg={arg} popover={false} />
                {index < filteredArgs.length - 1 && <span>, </span>}
              </div>
            ))}
            <ClosingParenthesis callsQuantity={callsQuantity} noQuantity />
          </PopoverContent>
        </Popover>
        <div className="grid items-center min-w-0" style={argsGridStyle}>
          {filteredArgs.map((arg, index) => (
            <Fragment key={index}>
              <Argument arg={arg} />
              {index < filteredArgs.length - 1 && <span>, </span>}
            </Fragment>
          ))}
        </div>
        <ClosingParenthesis callsQuantity={callsQuantity} />
      </div>
    )
  },
)
FunctionCall.displayName = 'FunctionCall'
