"""Tests for step() builder function."""

from motia import StepConfig, queue
from motia.step import StepBuilder, StepDefinition, step


class TestStep:
    """Tests for the step() builder."""

    def test_step_with_handler(self):
        """step(config, handler) returns StepDefinition."""
        config = {"name": "test", "triggers": [queue("test.topic")]}

        async def handler(data, ctx):
            pass

        result = step(config, handler)
        assert isinstance(result, StepDefinition)
        assert result.config.name == "test"
        assert result.handler is handler

    def test_step_without_handler_returns_builder(self):
        """step(config) returns StepBuilder with handle() method."""
        config = {"name": "test", "triggers": [queue("test.topic")]}
        builder = step(config)
        assert isinstance(builder, StepBuilder)
        assert hasattr(builder, "config")
        assert hasattr(builder, "handle")
        assert builder.config.name == "test"

    def test_step_builder_handle(self):
        """StepBuilder.handle() returns StepDefinition."""
        config = {"name": "test", "triggers": [queue("test.topic")]}

        async def handler(data, ctx):
            pass

        result = step(config).handle(handler)
        assert isinstance(result, StepDefinition)
        assert result.config.name == "test"
        assert result.handler is handler

    def test_step_accepts_dict_config(self):
        """step() accepts dict config and converts to StepConfig."""
        config = {"name": "test", "triggers": [queue("test.topic")], "enqueues": ["other.topic"]}

        async def handler(data, ctx):
            pass

        result = step(config, handler)
        assert isinstance(result.config, StepConfig)
        assert result.config.name == "test"

    def test_step_accepts_step_config(self):
        """step() accepts StepConfig instance directly."""
        config = StepConfig(name="test", triggers=[queue("test.topic")])

        async def handler(data, ctx):
            pass

        result = step(config, handler)
        assert result.config is config
