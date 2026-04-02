from __future__ import annotations

from typing import Any

from iii import IIIClient
from iii.stream import (
    IStream,
    StreamDeleteInput,
    StreamGetInput,
    StreamListGroupsInput,
    StreamListInput,
    StreamSetInput,
    StreamSetResult,
    StreamUpdateInput,
)

from .models import Todo


class StreamClient:
    def __init__(self, iii: IIIClient) -> None:
        self._iii = iii

    async def get(self, stream_name: str, group_id: str, item_id: str) -> Any | None:
        return await self._iii.trigger({
            "function_id": "stream::get",
            "payload": {"stream_name": stream_name, "group_id": group_id, "item_id": item_id},
        })

    async def set(self, stream_name: str, group_id: str, item_id: str, data: Any) -> Any:
        return await self._iii.trigger({
            "function_id": "stream::set",
            "payload": {"stream_name": stream_name, "group_id": group_id, "item_id": item_id, "data": data},
        })

    async def delete(self, stream_name: str, group_id: str, item_id: str) -> None:
        return await self._iii.trigger({
            "function_id": "stream::delete",
            "payload": {"stream_name": stream_name, "group_id": group_id, "item_id": item_id},
        })

    async def get_group(self, stream_name: str, group_id: str) -> list[Any]:
        return await self._iii.trigger({
            "function_id": "stream::list",
            "payload": {"stream_name": stream_name, "group_id": group_id},
        })

    async def list_groups(self, stream_name: str) -> list[str]:
        return await self._iii.trigger({"function_id": "stream::list_groups", "payload": {"stream_name": stream_name}})


class TodoStream(IStream[dict[str, Any]]):
    def __init__(self) -> None:
        self._todos: list[Todo] = []

    async def get(self, input: StreamGetInput) -> dict[str, Any] | None:
        for todo in self._todos:
            if todo.id == input.item_id:
                return todo.model_dump()
        return None

    async def set(self, input: StreamSetInput) -> StreamSetResult[dict[str, Any]] | None:
        for i, todo in enumerate(self._todos):
            if todo.id == input.item_id:
                updated = Todo(**{**todo.model_dump(), **input.data})
                self._todos[i] = updated
                return StreamSetResult(old_value=todo.model_dump(), new_value=updated.model_dump())

        new_todo = Todo(
            id=input.item_id,
            group_id=input.group_id,
            description=input.data.get("description", ""),
            due_date=input.data.get("dueDate"),
            completed_at=None,
        )
        self._todos.append(new_todo)
        return StreamSetResult(old_value=None, new_value=new_todo.model_dump())

    async def delete(self, input: StreamDeleteInput) -> None:
        self._todos = [t for t in self._todos if t.id != input.item_id]

    async def list(self, input: StreamListInput) -> list[dict[str, Any]]:
        return [t.model_dump() for t in self._todos if t.group_id == input.group_id]

    async def list_groups(self, input: StreamListGroupsInput) -> list[str]:
        return list({t.group_id for t in self._todos})

    async def update(self, input: StreamUpdateInput) -> StreamSetResult[dict[str, Any]] | None:
        return None


def register_streams(iii: IIIClient) -> None:
    iii.create_stream("todo", TodoStream())
