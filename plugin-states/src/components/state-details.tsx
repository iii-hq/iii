import type React from 'react'
import JsonView from 'react18-json-view'
import type { StateItem } from '../types/state'

type Props = {
  state: StateItem
}

export const StateDetails: React.FC<Props> = ({ state }) => <JsonView src={state.value} theme="default" />
