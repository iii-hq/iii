from pydantic import BaseModel


class Todo(BaseModel):
    id: str
    group_id: str
    description: str
    due_date: str | None = None
    completed_at: str | None = None
