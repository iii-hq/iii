"""A namespace that was declared and left blank is a mistake, not a way to ask
for ``default``.

Absent and blank mean opposite things. Read as the same, they produce the
failure nobody can see: the worker registers in ``default``, and since a
worker's calls and triggers follow its namespace, the whole project serves from
a place the declaration never named.

Checked before any connection, so it fails at startup the way ``iii compose``
refuses ``--ns ""``.
"""

import pytest

from iii import InitOptions
from iii.iii import III


def _client(namespace: str | None) -> III:
    # `III.__new__` skips __init__, so no connection thread is started for a
    # check that happens before any I/O. Same shape as test_worker_metadata.
    stub = III.__new__(III)
    options = InitOptions(worker_name="tester")
    if namespace is not None:
        options.namespace = namespace
    stub._options = options
    return stub


@pytest.mark.parametrize("blank", ["", "   ", "\t"])
def test_a_blank_option_is_refused(blank: str) -> None:
    client = _client(blank)
    with pytest.raises(ValueError, match="namespace is empty"):
        client._worker_namespace()


def test_a_blank_env_var_is_left_alone(monkeypatch: pytest.MonkeyPatch) -> None:
    # ``FOO=`` is how a shell says "not set", so a blank env var reads as absent
    # rather than being refused. Absent is a namespace a worker may
    # legitimately have none of; an option written and left empty is not.
    monkeypatch.setenv("III_NAMESPACE", "")
    assert _client(None)._worker_namespace() is None


def test_an_unset_env_var_is_still_no_namespace(monkeypatch: pytest.MonkeyPatch) -> None:
    # The whole fleet runs this way: no namespace declared anywhere, and the
    # engine applies its own default. That must keep working.
    monkeypatch.delenv("III_NAMESPACE", raising=False)
    assert _client(None)._worker_namespace() is None


def test_a_named_namespace_still_resolves(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("III_NAMESPACE", raising=False)
    assert _client("orders")._worker_namespace() == "orders"

    monkeypatch.setenv("III_NAMESPACE", "billing")
    assert _client(None)._worker_namespace() == "billing"
    # An explicit option still wins over the managed environment.
    assert _client("orders")._worker_namespace() == "orders"


class TestBlankCallNamespace:
    """The same rule on the per-call path.

    Python's ``or`` treats ``""`` as falsy, so a blank namespace here was
    silently replaced by the worker's while Node and Rust forwarded it and Go
    dropped it -- one mistake, four behaviours, none of them chosen.
    """

    @pytest.mark.parametrize("blank", ["", "   ", "\t"])
    def test_a_blank_one_is_refused(self, blank: str) -> None:
        client = _client("orders")
        with pytest.raises(ValueError, match="namespace is empty"):
            client._call_namespace(blank, "TriggerRequest.namespace")

    def test_the_message_names_the_field(self) -> None:
        client = _client("orders")
        with pytest.raises(ValueError, match="TriggerRequest.namespace"):
            client._call_namespace("", "TriggerRequest.namespace")

    def test_an_absent_one_inherits_the_workers(self) -> None:
        # The control: absent is not blank, and still means this worker's.
        client = _client("orders")
        assert client._call_namespace(None, "TriggerRequest.namespace") == "orders"

    def test_an_explicit_one_still_wins(self) -> None:
        client = _client("orders")
        assert client._call_namespace("billing", "TriggerRequest.namespace") == "billing"


class TestInvocationNamespace:
    def test_an_implicit_engine_call_stays_in_default(self) -> None:
        client = _client("orders")
        assert client._invocation_namespace(None, "engine::channels::create") == "default"

    def test_an_explicit_namespace_wins_for_an_engine_call(self) -> None:
        client = _client("orders")
        assert (
            client._invocation_namespace("sandbox", "engine::channels::create")
            == "sandbox"
        )

    def test_a_non_engine_call_inherits_the_worker_namespace(self) -> None:
        client = _client("orders")
        assert client._invocation_namespace(None, "router::chat") == "orders"
