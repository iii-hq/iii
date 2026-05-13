from __future__ import annotations

import os
import threading

from iii import InitOptions, register_worker


def main() -> None:
    engine_ws_url = os.environ.get("III_URL", "ws://localhost:49134")

    iii = register_worker(
        address=engine_ws_url,
        options=InitOptions(worker_name="my-worker"),
    )

    async def hello(data: dict) -> dict:
        return {"greeting": f"hello, {data.get('name', 'world')}"}

    iii.register_function("hello", hello)

    print(f"worker ready (engine: {engine_ws_url})")
    threading.Event().wait()


if __name__ == "__main__":
    main()
