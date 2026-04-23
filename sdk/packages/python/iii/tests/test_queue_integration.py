"""Integration tests for queue enqueue and void trigger actions."""

import time

from iii import TriggerAction
from iii.iii import III


def wait_for(condition, timeout=5.0, interval=0.1):
    """Poll until condition() is truthy or timeout."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if condition():
            return
        time.sleep(interval)
    raise TimeoutError(f"Condition not met within {timeout}s")


def _wait_for_registration(iii_client: III, function_id: str, timeout: float = 5.0) -> None:
    """Poll engine::functions::list until function_id is registered, or TimeoutError."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = iii_client.trigger({"function_id": "engine::functions::list", "payload": {}})
            functions = result.get("functions", []) if isinstance(result, dict) else []
            ids = [f.get("function_id") for f in functions if isinstance(f, dict)]
            if function_id in ids:
                return
        except Exception:
            pass
        time.sleep(0.1)
    raise TimeoutError(f"Function {function_id} was not registered within {timeout}s")


def test_enqueue_delivers_message_to_function(iii_client: III):
    """Enqueue action delivers payload to the registered consumer function."""
    received = []

    def consumer_handler(input_data):
        received.append(input_data)
        return None

    ref = iii_client.register_function({"id": "test.queue.py.consumer"}, consumer_handler)
    time.sleep(0.3)

    try:

        result = iii_client.trigger({
            "function_id": "test.queue.py.consumer",
            "payload": {"order": "pizza"},
            "action": TriggerAction.Enqueue(queue="test-orders"),
        })

        assert isinstance(result, dict)
        assert isinstance(result["messageReceiptId"], str)

        wait_for(lambda: len(received) > 0, timeout=5.0)

        assert received[0]["order"] == "pizza"
    finally:
        ref.unregister()


def test_void_trigger_returns_none(iii_client: III):
    """Void trigger action fires without waiting and returns None."""
    calls = []

    def void_consumer_handler(input_data):
        calls.append(input_data)
        return None

    ref = iii_client.register_function({"id": "test.queue.py.void-consumer"}, void_consumer_handler)
    time.sleep(0.3)

    try:
        result = iii_client.trigger({
            "function_id": "test.queue.py.void-consumer",
            "payload": {"msg": "fire"},
            "action": TriggerAction.Void(),
        })

        assert result is None

        wait_for(lambda: len(calls) > 0, timeout=5.0)

        assert calls[0]["msg"] == "fire"
    finally:
        ref.unregister()


def test_enqueue_multiple_messages(iii_client: III):
    """Multiple enqueued messages are all delivered to the consumer."""
    received = []

    def multi_handler(input_data):
        received.append(input_data)
        return None

    ref = iii_client.register_function({"id": "test.queue.py.multi"}, multi_handler)
    time.sleep(0.3)

    try:
        for i in range(5):
            iii_client.trigger({
                "function_id": "test.queue.py.multi",
                "payload": {"index": i},
                "action": TriggerAction.Enqueue(queue="test-multi"),
            })

        wait_for(lambda: len(received) == 5, timeout=10.0)

        assert len(received) == 5
        received_indices = sorted(item["index"] for item in received)
        assert received_indices == [0, 1, 2, 3, 4]
    finally:
        ref.unregister()


def test_chained_enqueue(iii_client: III):
    """Function A enqueues a message to function B, forming a chain."""
    chain_received = []

    def chain_a_handler(input_data):
        iii_client.trigger({
            "function_id": "test.queue.py.chain-b",
            "payload": {**input_data, "chained": True},
            "action": TriggerAction.Enqueue(queue="test-chain"),
        })
        return input_data

    def chain_b_handler(input_data):
        chain_received.append(input_data)
        return None

    ref_a = iii_client.register_function({"id": "test.queue.py.chain-a"}, chain_a_handler)
    ref_b = iii_client.register_function({"id": "test.queue.py.chain-b"}, chain_b_handler)
    time.sleep(0.3)

    try:
        iii_client.trigger({
            "function_id": "test.queue.py.chain-a",
            "payload": {"origin": "test"},
            "action": TriggerAction.Enqueue(queue="test-chain"),
        })

        wait_for(lambda: len(chain_received) > 0, timeout=5.0)

        assert chain_received[0]["chained"] is True
        assert chain_received[0]["origin"] == "test"
    finally:
        ref_b.unregister()
        ref_a.unregister()


# ---------------------------------------------------------------------------
# Durable subscriber / publisher scenarios (ported from motia-py, MOT-3109).
# These exercise the `durable:subscriber` trigger + `iii::durable::publish`
# path, complementing the TriggerAction.Enqueue tests above.
# ---------------------------------------------------------------------------


def test_durable_publish_delivers_to_subscribed_handler(iii_client: III):
    """Publishing to a topic delivers the message to a subscribed handler."""
    function_id = f"test.queue.py.durable.basic.{int(time.time() * 1000)}"
    topic = f"test-topic-{int(time.time() * 1000)}"
    received: list = []

    def handler(data):
        received.append(data)
        return None

    fn_ref = iii_client.register_function({"id": function_id}, handler)
    trigger = iii_client.register_trigger(
        {"type": "durable:subscriber", "function_id": function_id, "config": {"topic": topic}}
    )
    time.sleep(0.5)

    try:
        iii_client.trigger(
            {
                "function_id": "iii::durable::publish",
                "payload": {"topic": topic, "data": {"order": "abc"}},
            }
        )
        wait_for(lambda: len(received) > 0, timeout=5.0)
        assert received == [{"order": "abc"}]
    finally:
        trigger.unregister()
        fn_ref.unregister()


def test_durable_publish_delivers_exact_payload(iii_client: III):
    """Handler receives the exact published payload without mutation."""
    function_id = f"test.queue.py.durable.payload.{int(time.time() * 1000)}"
    topic = f"test-topic-payload-{int(time.time() * 1000)}"
    payload = {"id": "x1", "count": 42, "nested": {"a": 1}}
    received: list = []

    def handler(data):
        received.append(data)
        return None

    fn_ref = iii_client.register_function({"id": function_id}, handler)
    trigger = iii_client.register_trigger(
        {"type": "durable:subscriber", "function_id": function_id, "config": {"topic": topic}}
    )
    time.sleep(0.5)

    try:
        iii_client.trigger(
            {
                "function_id": "iii::durable::publish",
                "payload": {"topic": topic, "data": payload},
            }
        )
        wait_for(lambda: len(received) > 0, timeout=5.0)
        assert received == [payload]
    finally:
        trigger.unregister()
        fn_ref.unregister()


def test_durable_subscriber_with_queue_config(iii_client: III):
    """Subscription with queue_config (maxRetries, concurrency, type) still delivers."""
    function_id = f"test.queue.py.durable.infra.{int(time.time() * 1000)}"
    topic = f"test-topic-infra-{int(time.time() * 1000)}"
    received: list = []

    def handler(data):
        received.append(data)
        return None

    fn_ref = iii_client.register_function({"id": function_id}, handler)
    trigger = iii_client.register_trigger(
        {
            "type": "durable:subscriber",
            "function_id": function_id,
            "config": {
                "topic": topic,
                "queue_config": {
                    "maxRetries": 5,
                    "type": "standard",
                    "concurrency": 2,
                },
            },
        }
    )
    time.sleep(0.5)

    try:
        iii_client.trigger(
            {
                "function_id": "iii::durable::publish",
                "payload": {"topic": topic, "data": {"infra": True}},
            }
        )
        wait_for(lambda: len(received) > 0, timeout=5.0)
        assert received == [{"infra": True}]
    finally:
        trigger.unregister()
        fn_ref.unregister()


def test_durable_multiple_subscribers_fanout(iii_client: III):
    """Multiple subscribers on the same topic each receive every message."""
    topic = f"test-topic-multi-{int(time.time() * 1000)}"
    function_id1 = f"test.queue.py.durable.multi1.{int(time.time() * 1000)}"
    function_id2 = f"test.queue.py.durable.multi2.{int(time.time() * 1000)}"
    received1: list = []
    received2: list = []

    def handler1(data):
        received1.append(data)
        return None

    def handler2(data):
        received2.append(data)
        return None

    fn1 = iii_client.register_function({"id": function_id1}, handler1)
    fn2 = iii_client.register_function({"id": function_id2}, handler2)
    trigger1 = iii_client.register_trigger(
        {"type": "durable:subscriber", "function_id": function_id1, "config": {"topic": topic}}
    )
    trigger2 = iii_client.register_trigger(
        {"type": "durable:subscriber", "function_id": function_id2, "config": {"topic": topic}}
    )
    time.sleep(0.5)

    try:
        iii_client.trigger(
            {"function_id": "iii::durable::publish", "payload": {"topic": topic, "data": {"msg": 1}}}
        )
        iii_client.trigger(
            {"function_id": "iii::durable::publish", "payload": {"topic": topic, "data": {"msg": 2}}}
        )
        wait_for(lambda: len(received1) >= 2 and len(received2) >= 2, timeout=8.0)

        assert len(received1) == 2
        assert len(received2) == 2
        assert {"msg": 1} in received1
        assert {"msg": 2} in received1
        assert {"msg": 1} in received2
        assert {"msg": 2} in received2
    finally:
        trigger1.unregister()
        trigger2.unregister()
        fn1.unregister()
        fn2.unregister()


def test_durable_condition_function_filters_messages(iii_client: III):
    """Registering a condition_function_id filters out non-matching messages."""
    topic = f"test-topic-cond-{int(time.time() * 1000)}"
    function_id = f"test.queue.py.durable.cond.{int(time.time() * 1000)}"
    condition_path = f"{function_id}::conditions::0"
    handler_calls: list = []

    def handler(data):
        handler_calls.append(data)
        return None

    def condition(input_data):
        return bool(input_data.get("accept"))

    fn_ref = iii_client.register_function({"id": function_id}, handler)
    cond_ref = iii_client.register_function({"id": condition_path}, condition)
    trigger = iii_client.register_trigger(
        {
            "type": "durable:subscriber",
            "function_id": function_id,
            "config": {
                "topic": topic,
                "condition_function_id": condition_path,
            },
        }
    )

    try:
        _wait_for_registration(iii_client, function_id)
        _wait_for_registration(iii_client, condition_path)

        iii_client.trigger(
            {"function_id": "iii::durable::publish", "payload": {"topic": topic, "data": {"accept": False}}}
        )
        iii_client.trigger(
            {"function_id": "iii::durable::publish", "payload": {"topic": topic, "data": {"accept": True}}}
        )
        wait_for(lambda: len(handler_calls) >= 1, timeout=8.0)
        # Give the rejected message a moment to ensure it does NOT arrive.
        time.sleep(1.0)

        assert len(handler_calls) == 1
        assert handler_calls[0] == {"accept": True}
    finally:
        trigger.unregister()
        cond_ref.unregister()
        fn_ref.unregister()
