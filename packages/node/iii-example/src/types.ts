export interface Todo {
  id: string
  groupId?: string
  title: string
  description?: string
  createdAt: string
  updatedAt?: string
  dueDate?: string | null
  priority?: 'low' | 'medium' | 'high'
  tags?: string[]
  assignee?: string | null
  status?: 'pending' | 'in_progress' | 'completed' | 'overdue'
  completedAt?: string | null
  archivedAt?: string | null
  list?: string
  type?: 'todo' | 'project'
  subtasks?: string[]
  parentId?: string
  progress?: number
}

export interface TodoEvent {
  topic: string
  data: {
    todo?: Todo
    todoId?: string
    previousList?: string
    timestamp: string
    [key: string]: any
  }
}

export interface CronEvent {
  trigger: 'cron'
  job_id: string
  scheduled_time: string
  actual_time: string
}

export interface StreamEvent {
  stream_name: string
  user_id: string
  timestamp: string
}

export interface SearchQuery {
  query?: string
  tags?: string[]
  priority?: 'low' | 'medium' | 'high'
  assignee?: string
  status?: 'pending' | 'in_progress' | 'completed' | 'overdue'
}

export interface TodoStats {
  total: number
  overdue: number
  byList: Record<string, number>
  timestamp: string
}
