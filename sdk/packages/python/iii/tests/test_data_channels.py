"""Integration tests for data channel streaming between workers."""

import asyncio
import json
import time

from iii.iii import III
from iii.channels import ChannelReader, ChannelWriter


def test_stream_data_from_sender_to_processor(iii_client: III):
    """Sender writes records via a channel, processor reads and computes stats."""

    async def processor_handler(input_data):
        reader: ChannelReader = input_data["reader"]
        label = input_data["label"]

        raw = await reader.read_all()
        records = json.loads(raw.decode("utf-8"))

        total = sum(r["value"] for r in records)
        max_val = max(r["value"] for r in records)
        min_val = min(r["value"] for r in records)

        return {
            "label": label,
            "messages": [
                {"type": "stat", "key": "count", "value": len(records)},
                {"type": "stat", "key": "sum", "value": total},
                {"type": "stat", "key": "average", "value": total / len(records)},
                {"type": "stat", "key": "min", "value": min_val},
                {"type": "stat", "key": "max", "value": max_val},
            ],
        }

    async def sender_handler(input_data):
        records = input_data["records"]
        channel = await iii_client.create_channel_async()

        payload = json.dumps(records).encode("utf-8")
        await channel.writer.write(payload)
        await channel.writer.close_async()

        result = await iii_client.trigger_async({
            "function_id": "test.data.processor",
            "payload": {
                "label": "metrics-batch",
                "reader": channel.reader_ref.model_dump(),
            },
        })

        return result

    proc_ref = iii_client.register_function({"id": "test.data.processor"}, processor_handler)
    sender_ref = iii_client.register_function({"id": "test.data.sender"}, sender_handler)

    time.sleep(0.3)

    try:
        records = [
            {"name": "cpu_usage", "value": 72},
            {"name": "memory_mb", "value": 2048},
            {"name": "disk_iops", "value": 340},
            {"name": "network_mbps", "value": 95},
            {"name": "latency_ms", "value": 12},
        ]

        result = iii_client.trigger({
            "function_id": "test.data.sender",
            "payload": {"records": records},
        })

        assert result["label"] == "metrics-batch"
        assert len(result["messages"]) == 5

        msgs = {m["key"]: m["value"] for m in result["messages"]}
        assert msgs["count"] == 5
        assert msgs["sum"] == 2567
        assert msgs["average"] == 513.4
        assert msgs["min"] == 12
        assert msgs["max"] == 2048
    finally:
        sender_ref.unregister()
        proc_ref.unregister()


def test_bidirectional_streaming(iii_client: III):
    """Worker reads input, sends progress messages, writes binary result back."""

    async def worker_handler(input_data):
        reader: ChannelReader = input_data["reader"]
        writer: ChannelWriter = input_data["writer"]

        chunks = []
        chunk_count = 0
        async for chunk in reader:
            chunks.append(chunk)
            chunk_count += 1
            await writer.send_message_async(
                json.dumps(
                    {
                        "type": "progress",
                        "chunks_received": chunk_count,
                    }
                )
            )

        full_data = b"".join(chunks).decode("utf-8")
        words = full_data.split()

        await writer.send_message_async(
            json.dumps(
                {
                    "type": "complete",
                    "word_count": len(words),
                    "byte_count": len(b"".join(chunks)),
                }
            )
        )

        result_json = json.dumps(
            {
                "words": words[:5],
                "total": len(words),
            }
        ).encode("utf-8")
        await writer.write(result_json)
        await writer.close_async()

        return {"status": "done"}

    async def coordinator_handler(input_data):
        text = input_data["text"]
        chunk_size = input_data["chunkSize"]

        input_channel = await iii_client.create_channel_async()
        output_channel = await iii_client.create_channel_async()

        messages = []
        output_channel.reader.on_message(lambda msg: messages.append(json.loads(msg)))

        text_bytes = text.encode("utf-8")
        offset = 0
        while offset < len(text_bytes):
            end = min(offset + chunk_size, len(text_bytes))
            await input_channel.writer.write(text_bytes[offset:end])
            offset = end
        await input_channel.writer.close_async()

        worker_result = await iii_client.trigger_async({
            "function_id": "test.stream.worker",
            "payload": {
                "reader": input_channel.reader_ref.model_dump(),
                "writer": output_channel.writer_ref.model_dump(),
            },
        })

        result_data = await output_channel.reader.read_all()
        binary_result = json.loads(result_data.decode("utf-8"))

        return {
            "messages": messages,
            "binaryResult": binary_result,
            "workerResult": worker_result,
        }

    worker_ref = iii_client.register_function({"id": "test.stream.worker"}, worker_handler)
    coord_ref = iii_client.register_function({"id": "test.stream.coordinator"}, coordinator_handler)

    time.sleep(0.3)

    try:
        text = "The quick brown fox jumps over the lazy dog and then runs around the park"

        result = iii_client.trigger({
            "function_id": "test.stream.coordinator",
            "payload": {
                "text": text,
                "chunkSize": 10,
            },
        })

        progress_msgs = [m for m in result["messages"] if m["type"] == "progress"]
        complete_msg = next((m for m in result["messages"] if m["type"] == "complete"), None)

        assert len(progress_msgs) > 0
        assert complete_msg is not None
        assert complete_msg["word_count"] == len(text.split())

        assert result["binaryResult"]["total"] == len(text.split())
        assert len(result["binaryResult"]["words"]) == 5
        assert result["binaryResult"]["words"] == ["The", "quick", "brown", "fox", "jumps"]

        assert result["workerResult"]["status"] == "done"
    finally:
        coord_ref.unregister()
        worker_ref.unregister()
