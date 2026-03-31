"""
Pattern: HTTP Endpoints
Comparable to: Express, Fastify, Flask

Exposes RESTful HTTP endpoints backed by iii functions.
Each handler receives an ApiRequest object and returns
{ status_code, body, headers }.

How-to references:
  - HTTP endpoints: https://iii.dev/docs/how-to/expose-http-endpoint
"""

import asyncio
import os
import time
from datetime import datetime, timezone

from iii import InitOptions, Logger, TriggerAction, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="http-endpoints"),
)

# ---
# POST /users — Create a new user
# ApiRequest: { body, path_params, headers, method }
# ---


async def users_create(req):
    logger = Logger()
    name = req["body"]["name"]
    email = req["body"]["email"]
    id = f"usr-{int(time.time() * 1000)}"

    user = {"id": id, "name": name, "email": email, "created_at": datetime.now(timezone.utc).isoformat()}

    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {"scope": "users", "key": id, "value": user},
    })

    logger.info("User created", {"id": id, "email": email})

    return {"status_code": 201, "body": user, "headers": {"Content-Type": "application/json"}}


iii.register_function("users::create", users_create)

# ---
# GET /users/:id — Retrieve a user by path parameter
# ---


async def users_get_by_id(req):
    id = req["path_params"]["id"]

    user = await iii.trigger_async({
        "function_id": "state::get",
        "payload": {"scope": "users", "key": id},
    })

    if not user:
        return {"status_code": 404, "body": {"error": "User not found"}}

    return {"status_code": 200, "body": user}


iii.register_function("users::get-by-id", users_get_by_id)

# ---
# GET /users — List all users
# ---


async def users_list(data):
    users = await iii.trigger_async({
        "function_id": "state::list",
        "payload": {"scope": "users"},
    })

    return {"status_code": 200, "body": users}


iii.register_function("users::list", users_list)

# ---
# PUT /users/:id — Update an existing user
# ---


async def users_update(req):
    id = req["path_params"]["id"]
    updates = req["body"]

    existing = await iii.trigger_async({
        "function_id": "state::get",
        "payload": {"scope": "users", "key": id},
    })

    if not existing:
        return {"status_code": 404, "body": {"error": "User not found"}}

    ops = [{"type": "set", "path": path, "value": value} for path, value in updates.items()]
    ops.append({"type": "set", "path": "updated_at", "value": datetime.now(timezone.utc).isoformat()})

    await iii.trigger_async({
        "function_id": "state::update",
        "payload": {"scope": "users", "key": id, "ops": ops},
    })

    return {"status_code": 200, "body": {"id": id, **updates}}


iii.register_function("users::update", users_update)

# ---
# DELETE /users/:id — Remove a user
# ---


async def users_delete(req):
    id = req["path_params"]["id"]

    existing = await iii.trigger_async({
        "function_id": "state::get",
        "payload": {"scope": "users", "key": id},
    })

    if not existing:
        return {"status_code": 404, "body": {"error": "User not found"}}

    await iii.trigger_async({
        "function_id": "state::delete",
        "payload": {"scope": "users", "key": id},
    })

    return {"status_code": 204, "body": None}


iii.register_function("users::delete", users_delete)

# ---
# HTTP trigger registrations
# ---
iii.register_trigger({"type": "http", "function_id": "users::create", "config": {"api_path": "/users", "http_method": "POST"}})
iii.register_trigger({"type": "http", "function_id": "users::get-by-id", "config": {"api_path": "/users/:id", "http_method": "GET"}})
iii.register_trigger({"type": "http", "function_id": "users::list", "config": {"api_path": "/users", "http_method": "GET"}})
iii.register_trigger({"type": "http", "function_id": "users::update", "config": {"api_path": "/users/:id", "http_method": "PUT"}})
iii.register_trigger({"type": "http", "function_id": "users::delete", "config": {"api_path": "/users/:id", "http_method": "DELETE"}})


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
