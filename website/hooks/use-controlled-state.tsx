import * as React from 'react'

interface CommonControlledStateProps<T> {
  value?: T
  defaultValue?: T
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function useControlledState<T, Rest extends any[] = []>(
  props: CommonControlledStateProps<T> & {
    onChange?: (value: T, ...args: Rest) => void
  },
): readonly [T, (next: T, ...args: Rest) => void] {
  const { value, defaultValue, onChange } = props

  const [state, setInternalState] = React.useState<T>(value !== undefined ? value : (defaultValue as T))

  React.useEffect(() => {
    if (value !== undefined) setInternalState(value)
  }, [value])

  const setState = React.useCallback(
    (next: T, ...args: Rest) => {
      setInternalState(next)
      onChange?.(next, ...args)
    },
    [onChange],
  )

  return [state, setState] as const
}
