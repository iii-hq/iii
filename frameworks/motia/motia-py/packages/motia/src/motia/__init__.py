"""Motia framework for III Engine."""

try:
    from iii import ChannelReader, ChannelWriter
except ImportError:
    ChannelReader = None
    ChannelWriter = None

from . import tracing
from .enqueue import enqueue
from .guards import (
    get_api_triggers,
    get_cron_triggers,
    get_queue_triggers,
    get_state_triggers,
    get_stream_triggers,
    is_api_step,
    is_api_trigger,
    is_cron_step,
    is_cron_trigger,
    is_queue_step,
    is_queue_trigger,
    is_state_step,
    is_state_trigger,
    is_stream_step,
    is_stream_trigger,
)
from .iii import get_instance, init_iii
from .loader import generate_step_id
from .logger import logger
from .multi_trigger import MultiTriggerStepBuilder, multi_trigger_step
from .runtime import Motia
from .schema_utils import schema_to_json_schema
from .setup_step_endpoint import setup_step_endpoint
from .state import StateManager, stateManager
from .step import StepBuilder, StepDefinition, step
from .streams import Stream
from .triggers import api, cron, http, queue, state, stream
from .types import (
    ApiMiddleware,
    ApiRequest,
    ApiResponse,
    ApiRouteMethod,
    ApiTrigger,
    CronTrigger,
    Enqueue,
    Enqueuer,
    FlowContext,
    MotiaHttpArgs,
    MotiaHttpRequest,
    MotiaHttpResponse,
    QueryParam,
    QueueTrigger,
    StateTrigger,
    StateTriggerInput,
    Step,
    StepConfig,
    StreamEvent,
    StreamTrigger,
    StreamTriggerInput,
    TriggerCondition,
    TriggerConfig,
    TriggerInfo,
    TriggerInput,
)
from .types_stream import StreamAuthInput, StreamAuthResult, StreamConfig, StreamSubscription

__all__ = [
    # III SDK
    "get_instance",
    "init_iii",
    "ChannelReader",
    "ChannelWriter",
    # Runtime
    "Motia",
    # Setup
    "setup_step_endpoint",
    "generate_step_id",
    # Triggers
    "api",
    "http",
    "queue",
    "cron",
    "state",
    "stream",
    # Schema utils
    "schema_to_json_schema",
    # Step builders
    "step",
    "StepDefinition",
    "StepBuilder",
    # Multi-trigger step builder
    "multi_trigger_step",
    "MultiTriggerStepBuilder",
    # Types
    "ApiMiddleware",
    "ApiRequest",
    "ApiResponse",
    "ApiRouteMethod",
    "MotiaHttpRequest",
    "MotiaHttpArgs",
    "MotiaHttpResponse",
    "ApiTrigger",
    "CronTrigger",
    "Enqueue",
    "Enqueuer",
    "FlowContext",
    "QueryParam",
    "QueueTrigger",
    "StateTrigger",
    "StateTriggerInput",
    "Step",
    "StepConfig",
    "StreamConfig",
    "StreamAuthInput",
    "StreamAuthResult",
    "StreamEvent",
    "StreamSubscription",
    "StreamTrigger",
    "StreamTriggerInput",
    "TriggerCondition",
    "TriggerConfig",
    "TriggerInfo",
    "TriggerInput",
    # Standalone utilities
    "enqueue",
    "logger",
    # Streams & State
    "Stream",
    "StateManager",
    "stateManager",
    # Guards - trigger level
    "is_api_trigger",
    "is_queue_trigger",
    "is_cron_trigger",
    "is_state_trigger",
    "is_stream_trigger",
    # Guards - step level
    "is_api_step",
    "is_queue_step",
    "is_cron_step",
    "is_state_step",
    "is_stream_step",
    # Guards - getters
    "get_api_triggers",
    "get_queue_triggers",
    "get_cron_triggers",
    "get_state_triggers",
    "get_stream_triggers",
    # Tracing
    "tracing",
]
