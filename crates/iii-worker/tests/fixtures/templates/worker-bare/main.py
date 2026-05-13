import asyncio

from iii_sdk import register_worker


async def main() -> None:
    await register_worker(name="my-worker")
    print("worker ready")


if __name__ == "__main__":
    asyncio.run(main())
