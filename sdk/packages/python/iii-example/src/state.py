from typing import Any

from iii import IIIClient


class State:
    def __init__(self, iii: IIIClient) -> None:
        self._iii = iii

    async def get(self, scope: str, key: str) -> Any | None:
        return await self._iii.trigger({"function_id": "state::get", "payload": {"scope": scope, "key": key}})

    async def set(self, scope: str, key: str, value: Any) -> Any:
        return await self._iii.trigger({"function_id": "state::set", "payload": {"scope": scope, "key": key, "value": value}})

    async def delete(self, scope: str, key: str) -> None:
        return await self._iii.trigger({"function_id": "state::delete", "payload": {"scope": scope, "key": key}})

    async def get_group(self, scope: str) -> list[Any]:
        return await self._iii.trigger({"function_id": "state::list", "payload": {"scope": scope}})
