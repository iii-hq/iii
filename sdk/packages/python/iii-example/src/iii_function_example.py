from iii import register_worker
from pydantic import BaseModel

iii = register_worker('ws://localhost:49134')

class Todo(BaseModel):
    id: str
    group_id: str
    description: str
    due_date: str | None = None
    completed_at: str | None = None

def create_todo(todo: Todo) -> Todo:
    return todo

iii.register_function("myscope::create_todo", create_todo)
