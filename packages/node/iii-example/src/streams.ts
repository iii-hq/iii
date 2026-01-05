import { bridge } from './bridge'
import { Todo } from './types'

export const streams = {
  get: async (stream_name: string, group_id: string, item_id: string): Promise<any | null> => {
    return bridge.invokeFunction('streams.get', { stream_name, group_id, item_id })
  },
  set: async (stream_name: string, group_id: string, item_id: string, data: any): Promise<any> => {
    return bridge.invokeFunction('streams.set', { stream_name, group_id, item_id, data })
  },
  delete: async (stream_name: string, group_id: string, item_id: string): Promise<void> => {
    return bridge.invokeFunction('streams.delete', { stream_name, group_id, item_id })
  },
  getGroup: async (stream_name: string, group_id: string): Promise<any[]> => {
    return bridge.invokeFunction('streams.getGroup', { stream_name, group_id })
  },
}

let todoState: Todo[] = []

bridge.createStream('todo', {
  get: async (input) => todoState.find((todo) => todo.id === input.item_id),
  set: async (input) => {
    const existingTodo = todoState.find((todo) => todo.id === input.item_id)

    if (existingTodo) {
      todoState = todoState.map((todo) => (todo.id === input.item_id ? { ...todo, ...input.data } : todo))
      return { existed: true, data: existingTodo }
    }

    const newTodo = {
      id: input.item_id,
      groupId: input.group_id,
      description: input.data.description,
      dueDate: input.data.dueDate,
      completedAt: null,
    }

    todoState.push(newTodo)

    return { existed: false, data: newTodo }
  },
  delete: async (input) => {
    todoState = todoState.filter((todo) => todo.id !== input.item_id)
  },
  getGroup: async (input) => todoState.filter((todo) => todo.groupId === input.group_id),
})
