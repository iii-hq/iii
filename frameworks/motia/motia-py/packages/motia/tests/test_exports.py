# motia/tests/test_exports.py
"""Tests for package exports."""


def test_state_trigger_exports():
    """Test state trigger types are exported."""
    from motia import StateTrigger, StateTriggerInput, state

    assert StateTrigger is not None
    assert StateTriggerInput is not None
    assert state is not None


def test_stream_trigger_exports():
    """Test stream trigger types are exported."""
    from motia import StreamEvent, StreamTrigger, StreamTriggerInput, stream

    assert StreamTrigger is not None
    assert StreamTriggerInput is not None
    assert StreamEvent is not None
    assert stream is not None


def test_tracing_exports():
    """Test tracing utilities are exported."""
    from motia import tracing

    assert hasattr(tracing, "HAS_OTEL")
    assert hasattr(tracing, "get_tracer")
    assert hasattr(tracing, "instrument_bridge")


def test_enqueue_type_exports():
    """Test Enqueue and Enqueuer types are exported."""
    from motia import Enqueue, Enqueuer

    assert Enqueue is not None
    assert Enqueuer is not None


def test_trigger_info_export():
    """Test TriggerInfo type is exported."""
    from motia import TriggerInfo

    assert TriggerInfo is not None


def test_iii_exports():
    """Test III SDK functions are exported."""
    from motia import get_instance, init_iii

    assert get_instance is not None
    assert init_iii is not None


def test_state_manager_export():
    """Test stateManager is exported."""
    from motia import stateManager

    assert stateManager is not None


def test_setup_step_endpoint_export():
    """Test setup_step_endpoint is exported."""
    from motia import setup_step_endpoint

    assert setup_step_endpoint is not None


def test_generate_step_id_export():
    """Test generate_step_id is exported."""
    from motia import generate_step_id

    assert generate_step_id is not None


def test_guard_trigger_level_exports():
    """Test trigger-level guard functions are exported."""
    from motia import is_api_trigger, is_cron_trigger, is_queue_trigger, is_state_trigger, is_stream_trigger

    assert is_api_trigger is not None
    assert is_queue_trigger is not None
    assert is_cron_trigger is not None
    assert is_state_trigger is not None
    assert is_stream_trigger is not None


def test_guard_step_level_exports():
    """Test step-level guard functions are exported."""
    from motia import is_queue_step

    assert is_queue_step is not None


def test_guard_getter_exports():
    """Test guard getter functions are exported."""
    from motia import get_api_triggers, get_cron_triggers, get_queue_triggers

    assert get_api_triggers is not None
    assert get_queue_triggers is not None
    assert get_cron_triggers is not None
