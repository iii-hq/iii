import { iii } from './iii'
import type { Todo } from './types'

export const streams = {
  get: async <T = unknown>(
    stream_name: string,
    group_id: string,
    item_id: string,
  ): Promise<T | null> =>
    iii.trigger({ function_id: 'stream::get', payload: { stream_name, group_id, item_id } }),
  set: async <T = unknown>(
    stream_name: string,
    group_id: string,
    item_id: string,
    data: T,
  ): Promise<T> =>
    iii.trigger({ function_id: 'stream::set', payload: { stream_name, group_id, item_id, data } }),
  delete: async (stream_name: string, group_id: string, item_id: string): Promise<void> =>
    iii.trigger({
      function_id: 'stream::delete',
      payload: { stream_name, group_id, item_id },
    }),
  list: async <T = unknown>(stream_name: string, group_id: string): Promise<T[]> =>
    iii.trigger({ function_id: 'stream::list', payload: { stream_name, group_id } }),
  listGroups: async (stream_name: string): Promise<string[]> =>
    iii.trigger({ function_id: 'stream::list_groups', payload: { stream_name } }),
}

let todoState: Todo[] = []

iii.createStream('todo', {
  get: async input => todoState.find(todo => todo.id === input.item_id),
  set: async input => {
    const existingTodo = todoState.find(todo => todo.id === input.item_id)

    if (existingTodo) {
      const newTodo = { ...existingTodo, ...input.data }
      todoState = todoState.map(todo => (todo.id === input.item_id ? newTodo : todo))
      return { old_value: existingTodo, new_value: newTodo }
    }

    const newTodo: Todo = {
      id: input.item_id,
      groupId: input.group_id,
      description: input.data.description,
      dueDate: input.data.dueDate,
      completedAt: null,
      createdAt: input.data?.createdAt ?? new Date().toISOString(),
    }

    todoState.push(newTodo)

    return { old_value: undefined, new_value: newTodo }
  },
  delete: async input => {
    const old_value = todoState.find(todo => todo.id === input.item_id)
    todoState = todoState.filter(todo => todo.id !== input.item_id)
    return { old_value }
  },
  list: async input => todoState.filter(todo => todo.groupId === input.group_id),
  listGroups: async () => [...new Set(todoState.map(todo => todo.groupId))],
  update: async () => {
    throw new Error('Not implemented')
  },
})
