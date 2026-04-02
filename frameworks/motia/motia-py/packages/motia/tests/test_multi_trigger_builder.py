"""Tests for multi_trigger_step() builder function."""

import pytest

from motia import FlowContext, TriggerInfo, api, cron, queue
from motia.multi_trigger import MultiTriggerStepBuilder, multi_trigger_step
from motia.step import StepDefinition


class TestMultiTriggerStep:
    """Tests for the multi_trigger_step() builder."""

    def test_handlers_dict_style(self):
        """handlers(dict) creates unified handler from dict."""
        config = {
            "name": "multi",
            "triggers": [queue("data.in"), api("POST", "/items")],
        }

        async def queue_handler(data, ctx):
            return "queue"

        async def http_handler(req, ctx):
            return "http"

        result = multi_trigger_step(config).handlers({
            "queue": queue_handler,
            "http": http_handler,
        })
        assert isinstance(result, StepDefinition)
        assert result.config.name == "multi"
        assert result.handler is not None

    def test_chainable_on_queue(self):
        """on_queue() chains and handlers() finalizes."""
        config = {
            "name": "multi",
            "triggers": [queue("data.in"), cron("0 * * * *")],
        }

        async def queue_handler(data, ctx):
            pass

        async def cron_handler(ctx):
            pass

        result = (
            multi_trigger_step(config)
            .on_queue(queue_handler)
            .on_cron(cron_handler)
            .handlers()
        )
        assert isinstance(result, StepDefinition)
        assert result.config.name == "multi"

    def test_chainable_on_http(self):
        """on_http() chains properly."""
        config = {
            "name": "multi",
            "triggers": [api("POST", "/items"), queue("items.created")],
        }

        async def http_handler(req, ctx):
            pass

        async def queue_handler(data, ctx):
            pass

        result = (
            multi_trigger_step(config)
            .on_http(http_handler)
            .on_queue(queue_handler)
            .handlers()
        )
        assert isinstance(result, StepDefinition)
        assert result.config.name == "multi"

    def test_returns_builder_type(self):
        """multi_trigger_step() returns MultiTriggerStepBuilder."""
        config = {"name": "multi", "triggers": [queue("data.in")]}
        builder = multi_trigger_step(config)
        assert isinstance(builder, MultiTriggerStepBuilder)

    @pytest.mark.asyncio
    async def test_unified_handler_dispatches_queue(self):
        """Unified handler dispatches to queue handler based on trigger type."""
        config = {"name": "multi", "triggers": [queue("data.in")]}
        results = []

        async def queue_handler(data, ctx):
            results.append(("queue", data))

        definition = multi_trigger_step(config).handlers({"queue": queue_handler})

        mock_ctx = FlowContext(
            enqueue=lambda x: None,
            trace_id="test",
            state=None,
            logger=None,
            trigger=TriggerInfo(type="queue"),
        )
        await definition.handler({"key": "value"}, mock_ctx)
        assert results == [("queue", {"key": "value"})]

    @pytest.mark.asyncio
    async def test_unified_handler_dispatches_cron(self):
        """Unified handler dispatches to cron handler."""
        config = {"name": "multi", "triggers": [cron("0 * * * *")]}
        results = []

        async def cron_handler(ctx):
            results.append("cron")

        definition = multi_trigger_step(config).handlers({"cron": cron_handler})

        mock_ctx = FlowContext(
            enqueue=lambda x: None,
            trace_id="test",
            state=None,
            logger=None,
            trigger=TriggerInfo(type="cron"),
        )
        await definition.handler(None, mock_ctx)
        assert results == ["cron"]

    @pytest.mark.asyncio
    async def test_unified_handler_raises_for_unhandled(self):
        """Unified handler raises when no handler matches trigger type."""
        config = {"name": "multi", "triggers": [queue("data.in")]}

        async def queue_handler(data, ctx):
            pass

        definition = multi_trigger_step(config).handlers({"queue": queue_handler})

        mock_ctx = FlowContext(
            enqueue=lambda x: None,
            trace_id="test",
            state=None,
            logger=None,
            trigger=TriggerInfo(type="cron"),
        )
        with pytest.raises(RuntimeError, match="No handler defined for trigger type"):
            await definition.handler(None, mock_ctx)
