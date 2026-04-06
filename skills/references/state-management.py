"""
Pattern: State Management
Comparable to: Redis, DynamoDB, Memcached

Persistent key-value state scoped by namespace. Supports set, get,
list, delete, and partial update operations.

How-to references:
  - State management: https://iii.dev/docs/how-to/manage-state
"""

import asyncio
import os
import time
from datetime import datetime, timezone

from iii import InitOptions, Logger, TriggerAction, register_worker

iii = register_worker(
    address=os.environ.get("III_ENGINE_URL", "ws://localhost:49134"),
    options=InitOptions(worker_name="state-management"),
)

# ---
# state::set — Store a value under a scoped key
# Payload: { scope, key, value }
# ---


async def products_create(data):
    id = f"prod-{int(time.time() * 1000)}"
    product = {
        "id": id,
        "name": data["name"],
        "price": data["price"],
        "category": data["category"],
        "stock": data.get("stock", 0),
        "created_at": datetime.now(timezone.utc).isoformat(),
    }

    await iii.trigger_async({
        "function_id": "state::set",
        "payload": {"scope": "products", "key": id, "value": product},
    })

    return product


iii.register_function("products::create", products_create)

# ---
# state::get — Retrieve a value by scope and key
# Payload: { scope, key }
# Returns None if the key does not exist — always guard for None.
# ---


async def products_get(data):
    product = await iii.trigger_async({
        "function_id": "state::get",
        "payload": {"scope": "products", "key": data["id"]},
    })

    if not product:
        return {"error": "Product not found", "id": data["id"]}

    return product


iii.register_function("products::get", products_get)

# ---
# state::list — Retrieve all values in a scope
# Payload: { scope }
# Returns an array of all stored values.
# ---


async def products_list_all(data):
    products = await iii.trigger_async({
        "function_id": "state::list",
        "payload": {"scope": "products"},
    })

    products = products or []
    return {"count": len(products), "products": products}


iii.register_function("products::list-all", products_list_all)

# ---
# state::delete — Remove a key from a scope
# Payload: { scope, key }
# ---


async def products_remove(data):
    existing = await iii.trigger_async({
        "function_id": "state::get",
        "payload": {"scope": "products", "key": data["id"]},
    })

    if not existing:
        return {"error": "Product not found", "id": data["id"]}

    await iii.trigger_async({
        "function_id": "state::delete",
        "payload": {"scope": "products", "key": data["id"]},
    })

    return {"deleted": data["id"]}


iii.register_function("products::remove", products_remove)

# ---
# state::update — Partial merge using ops array
# Payload: { scope, key, ops }
# ops: [{ type: "set", path, value }]
# Use update instead of get-then-set for atomic partial changes.
# ---


async def products_update_price(data):
    existing = await iii.trigger_async({
        "function_id": "state::get",
        "payload": {"scope": "products", "key": data["id"]},
    })

    if not existing:
        return {"error": "Product not found", "id": data["id"]}

    await iii.trigger_async({
        "function_id": "state::update",
        "payload": {
            "scope": "products",
            "key": data["id"],
            "ops": [
                {"type": "set", "path": "price", "value": data["newPrice"]},
                {"type": "set", "path": "updated_at", "value": datetime.now(timezone.utc).isoformat()},
            ],
        },
    })

    return {"id": data["id"], "price": data["newPrice"]}


iii.register_function("products::update-price", products_update_price)

# ---
# Combining operations — inventory adjustment with update
# ---


async def products_adjust_stock(data):
    logger = Logger()

    product = await iii.trigger_async({
        "function_id": "state::get",
        "payload": {"scope": "products", "key": data["id"]},
    })

    if not product:
        return {"error": "Product not found", "id": data["id"]}

    new_stock = product["stock"] + data["adjustment"]

    if new_stock < 0:
        return {"error": "Insufficient stock", "current": product["stock"], "requested": data["adjustment"]}

    await iii.trigger_async({
        "function_id": "state::update",
        "payload": {
            "scope": "products",
            "key": data["id"],
            "ops": [
                {"type": "set", "path": "stock", "value": new_stock},
                {"type": "set", "path": "last_stock_change", "value": datetime.now(timezone.utc).isoformat()},
            ],
        },
    })

    logger.info("Stock adjusted", {"id": data["id"], "from": product["stock"], "to": new_stock})
    return {"id": data["id"], "previousStock": product["stock"], "newStock": new_stock}


iii.register_function("products::adjust-stock", products_adjust_stock)

# ---
# HTTP triggers
# ---
iii.register_trigger({"type": "http", "function_id": "products::create", "config": {"api_path": "/products", "http_method": "POST"}})
iii.register_trigger({"type": "http", "function_id": "products::get", "config": {"api_path": "/products/:id", "http_method": "GET"}})
iii.register_trigger({"type": "http", "function_id": "products::list-all", "config": {"api_path": "/products", "http_method": "GET"}})
iii.register_trigger({"type": "http", "function_id": "products::remove", "config": {"api_path": "/products/:id", "http_method": "DELETE"}})
iii.register_trigger({"type": "http", "function_id": "products::update-price", "config": {"api_path": "/products/:id/price", "http_method": "PUT"}})
iii.register_trigger({"type": "http", "function_id": "products::adjust-stock", "config": {"api_path": "/products/:id/stock", "http_method": "POST"}})


async def main():
    while True:
        await asyncio.sleep(60)


if __name__ == "__main__":
    asyncio.run(main())
