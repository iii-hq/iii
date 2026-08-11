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


def test_a_blank_env_var_is_refused(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("III_NAMESPACE", "")
    client = _client(None)
    with pytest.raises(ValueError, match="III_NAMESPACE"):
        client._worker_namespace()


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
