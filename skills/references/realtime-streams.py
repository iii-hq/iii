"""
Pattern: Realtime Streams
Comparable to: Socket.io, Pusher, Firebase Realtime

Push live data to connected WebSocket clients.
Clients connect at: ws://host:3112/stream/{stream_name}/{group_id}

Built-in stream operations: stream::set, stream::get, stream::list,
stream::delete, stream::send.

Note: The Python SDK does not support createStream for custom adapters.
Use the built-in stream operations and state-backed presence instead.

How-to references:
  - Realtime streams: https://iii.dev/docs/how-to/stream-realtime-data
"""

import asyncio
import os
import uuid
from datetime import datetime, timezone

from iii import InitOptions, Logger, TriggerAction, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="realtime-streams"),
)

# ---
# stream::set — Persist an item in a stream group
# Payload: { stream_name, group_id, item_id, data }
# ---


async def post_message(data):
    logger = Logger()
    message_id = f"msg-{uuid.uuid4().hex}"

    await iii.trigger_async({
        "function_id": "stream::set",
        "payload": {
            "stream_name": "chat",
            "group_id": data["room"],
            "item_id": message_id,
            "data": {
                "sender": data["sender"],
                "text": data["text"],
                "timestamp": datetime.now(timezone.utc).isoformat(),
            },
        },
    })

    logger.info("Message stored in stream", {"room": data["room"], "messageId": message_id})
    return {"messageId": message_id}


iii.register_function("chat::post-message", post_message)

# ---
# stream::get — Retrieve a single item from a stream group
# Payload: { stream_name, group_id, item_id }
# ---


async def get_message(data):
    message = await iii.trigger_async({
        "function_id": "stream::get",
        "payload": {
            "stream_name": "chat",
            "group_id": data["room"],
            "item_id": data["messageId"],
        },
    })

    if not message:
        return {"error": "Message not found"}

    return message


iii.register_function("chat::get-message", get_message)

# ---
# stream::list — List all items in a stream group
# Payload: { stream_name, group_id }
# ---


async def list_messages(data):
    messages = await iii.trigger_async({
        "function_id": "stream::list",
        "payload": {
            "stream_name": "chat",
            "group_id": data["room"],
        },
    })

    return {"room": data["room"], "messages": messages or []}


iii.register_function("chat::list-messages", list_messages)

# ---
# stream::delete — Remove an item from a stream group
# Payload: { stream_name, group_id, item_id }
# ---


async def delete_message(data):
    await iii.trigger_async({
        "function_id": "stream::delete",
        "payload": {
            "stream_name": "chat",
            "group_id": data["room"],
            "item_id": data["messageId"],
        },
    })

    return {"deleted": data["messageId"]}


iii.register_function("chat::delete-message", delete_message)

# ---
# stream::send — Push a live event to all connected clients
# Clients on ws://host:3112/stream/chat/{room} receive this instantly.
# Use TriggerAction.Void() for fire-and-forget delivery.
# ---


async def broadcast(data):
    logger = Logger()
    event_id = f"evt-{uuid.uuid4().hex}"

    await iii.trigger_async({
        "function_id": "stream::set",
        "payload": {
            "stream_name": "chat",
            "group_id": data["room"],
            "item_id": event_id,
            "data": {
                "sender": data["sender"],
                "text": data["text"],
                "timestamp": datetime.now(timezone.utc).isoformat(),
            },
        },
    })

    iii.trigger({
        "function_id": "stream::send",
        "payload": {
            "stream_name": "chat",
            "group_id": data["room"],
            "id": event_id,
            "event_type": "new_message",
            "data": {
                "sender": data["sender"],
                "text": data["text"],
                "timestamp": datetime.now(timezone.utc).isoformat(),
            },
        },
        "action": TriggerAction.Void(),
    })

    logger.info("Message broadcast", {"room": data["room"], "eventId": event_id})
    return {"eventId": event_id}


iii.register_function("chat::broadcast", broadcast)

# ---
# Presence tracking — user joins/leaves
# Uses state-backed storage since Python SDK lacks createStream.
# Clients connect at: ws://host:3112/stream/presence/{room}
# ---


async def presence_join(data):
    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {
            "scope": f"presence::{data['room']}",
            "key": data["userId"],
            "value": {
                "userId": data["userId"],
                "name": data["name"],
                "status": "online",
                "updated_at": datetime.now(timezone.utc).isoformat(),
            },
        },
    })

    iii.trigger({
        "function_id": "stream::send",
        "payload": {
            "stream_name": "presence",
            "group_id": data["room"],
            "id": f"join-{uuid.uuid4().hex}",
            "event_type": "user_joined",
            "data": {"userId": data["userId"], "name": data["name"]},
        },
        "action": TriggerAction.Void(),
    })

    return {"joined": data["room"]}


iii.register_function("presence::join", presence_join)


async def presence_leave(data):
    await iii.trigger_async({
        "function_id": "state::delete",
        "payload": {
            "scope": f"presence::{data['room']}",
            "key": data["userId"],
        },
    })

    iii.trigger({
        "function_id": "stream::send",
        "payload": {
            "stream_name": "presence",
            "group_id": data["room"],
            "id": f"leave-{uuid.uuid4().hex}",
            "event_type": "user_left",
            "data": {"userId": data["userId"]},
        },
        "action": TriggerAction.Void(),
    })

    return {"left": data["room"]}


iii.register_function("presence::leave", presence_leave)

# ---
# HTTP triggers
# ---
iii.register_trigger({"type": "http", "function_id": "chat::broadcast", "config": {"api_path": "/chat/send", "http_method": "POST"}})
iii.register_trigger({"type": "http", "function_id": "chat::list-messages", "config": {"api_path": "/chat/:room/messages", "http_method": "GET"}})
iii.register_trigger({"type": "http", "function_id": "presence::join", "config": {"api_path": "/presence/join", "http_method": "POST"}})
iii.register_trigger({"type": "http", "function_id": "presence::leave", "config": {"api_path": "/presence/leave", "http_method": "POST"}})


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
