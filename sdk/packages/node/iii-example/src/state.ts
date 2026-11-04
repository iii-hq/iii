import type {
  IState,
  StateDeleteInput,
  StateDeleteResult,
  StateGetInput,
  StateListInput,
  StateSetInput,
  StateSetResult,
  StateUpdateInput,
  StateUpdateResult,
} from 'iii-sdk/state'
import { iii } from './iii'

export const state: IState = {
  get: <TData>(input: StateGetInput): Promise<TData | null> =>
    iii.call({ function_id: 'state::get', payload: input }),
  set: <TData>(input: StateSetInput): Promise<StateSetResult<TData> | null> =>
    iii.call({ function_id: 'state::set', payload: input }),
  delete: (input: StateDeleteInput): Promise<StateDeleteResult> =>
    iii.call({ function_id: 'state::delete', payload: input }),
  list: <TData>(input: StateListInput): Promise<TData[]> => iii.call({ function_id: 'state::list', payload: input }),
  update: <TData>(input: StateUpdateInput): Promise<StateUpdateResult<TData> | null> =>
    iii.call({ function_id: 'state::update', payload: input }),
}
