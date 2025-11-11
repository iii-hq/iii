import { Bridge } from 'iii'

const bridge = new Bridge(process.env.III_BRIDGE_URL ?? 'ws://localhost:49134')

export const useCallback = <TInput, TOutput>(functionPath: string, callback: (data: TInput) => Promise<TOutput>) => {
  return bridge.registerFunction({ functionPath }, callback)
}

export const useRemoteFunction =
  <TInput, TOutput>(functionId: string) =>
  (data: TInput) => {
    return bridge.invokeFunction<TInput, TOutput>(functionId, data)
  }

const useWorkflow = (workflow: () => void) => {
  //
}

const useState = <T>(initialState: T) => {
  return ['', () => {}]
}

useWorkflow(() => {
  const [state, setState] = useState<string>('')
})
