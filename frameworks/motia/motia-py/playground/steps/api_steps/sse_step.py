"""Accepts URL-encoded data and streams back random items as SSE."""

import json
import math
import random
import time
from typing import Any
from urllib.parse import parse_qsl

from motia import MotiaHttpArgs, http, logger

config = {
    "name": "SSE Example",
    "description": "Accepts URL-encoded data and streams back random items as SSE",
    "flows": ["sse-example"],
    "triggers": [
        http("POST", "/sse"),
    ],
    "enqueues": [],
}


async def handler(args: MotiaHttpArgs[Any]) -> None:
    """Read URL-encoded body, then stream random items back as SSE."""
    request = args.request
    response = args.response

    safe_header_keys = ("content-type", "user-agent", "accept", "content-length", "x-request-id")
    safe_headers = {k: v for k, v in request.headers.items() if k.lower() in safe_header_keys}
    logger.info("Data received", {"headers": safe_headers})

    await response.status(200)
    await response.headers({
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        "connection": "keep-alive",
    })

    raw_chunks: list[str] = []
    for chunk in request.request_body.stream:
        if isinstance(chunk, bytes):
            raw_chunks.append(chunk.decode("utf-8", errors="replace"))
        else:
            raw_chunks.append(str(chunk))

    payload = "".join(raw_chunks).strip()
    parts: dict[str, str] = dict(parse_qsl(payload, keep_blank_values=True))

    items = _generate_random_items(parts)

    for item in items:
        response.writer.stream.write(f"event: item\ndata: {json.dumps(item)}\n\n".encode("utf-8"))
        time.sleep(0.3 + random.random() * 0.7)

    response.writer.stream.write(f"event: done\ndata: {json.dumps({'total': len(items)})}\n\n".encode("utf-8"))
    response.close()


def _generate_random_items(parts: dict[str, str]) -> list[dict[str, Any]]:
    fields = [{"name": k, "value": v} for k, v in parts.items()]
    count = 5 + math.floor(random.random() * 6)

    adjectives = ["swift", "lazy", "bold", "calm", "fierce", "gentle", "sharp", "wild"]
    nouns = ["falcon", "river", "mountain", "crystal", "thunder", "shadow", "ember", "frost"]

    return [
        {
            "id": f"item-{int(time.time() * 1000)}-{i}",
            "label": f"{random.choice(adjectives)} {random.choice(nouns)}",
            "score": round(random.random() * 100),
            "source": fields[i % len(fields)] if fields else None,
        }
        for i in range(count)
    ]
