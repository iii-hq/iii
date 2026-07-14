"""iii stream helpers."""

from __future__ import annotations

from typing import Any, Generic, Literal, TypeVar

from pydantic import BaseModel, Field, model_serializer

__all__ = [
    "MergePath",
    "StreamAuthInput",
    "StreamAuthResult",
    "StreamChangeEvent",
    "StreamChangeEventDetail",
    "StreamContext",
    "StreamDeleteInput",
    "StreamDeleteResult",
    "StreamGetInput",
    "StreamJoinLeaveEvent",
    "StreamJoinLeaveTriggerConfig",
    "StreamJoinResult",
    "StreamListGroupsInput",
    "StreamListInput",
    "StreamSetInput",
    "StreamSetResult",
    "StreamTriggerConfig",
    "StreamUpdateInput",
    "StreamUpdateResult",
    "UpdateAppend",
    "UpdateDecrement",
    "UpdateIncrement",
    "UpdateMerge",
    "UpdateOp",
    "UpdateOpError",
    "UpdateRemove",
    "UpdateSet",
]

TData = TypeVar("TData")

# Path target for a ``merge`` / ``append`` op. Accepts either a single
# string (legacy / first-level key) or a list of literal segments
# (nested path). Mirrors the Node ``string | string[]`` alias and the
# Rust ``MergePath`` enum.
MergePath = str | list[str]


class StreamAuthInput(BaseModel):
    """Input for stream authentication.

    Attributes:
        headers: Request headers.
        path: Request path.
        query_params: Query parameters.
        addr: Client address.
    """

    headers: dict[str, str]
    path: str
    query_params: dict[str, list[str]]
    addr: str


class StreamAuthResult(BaseModel):
    """Result of stream authentication.

    Attributes:
        context: Arbitrary context passed to stream handlers after authentication.
    """

    context: Any | None = None


StreamContext = Any


class StreamJoinLeaveEvent(BaseModel):
    """Event for stream join/leave.

    Attributes:
        subscription_id: Unique subscription identifier.
        stream_name: Name of the stream.
        group_id: Group identifier.
        id: Item identifier (if applicable).
        context: Auth context from :class:`StreamAuthResult`.
    """

    subscription_id: str
    stream_name: str
    group_id: str
    id: str | None = None
    context: Any | None = None


class StreamJoinResult(BaseModel):
    """Result of stream join.

    Attributes:
        unauthorized: Whether the join was unauthorized.
    """

    unauthorized: bool


class StreamGetInput(BaseModel):
    """Input for stream get operation.

    Attributes:
        stream_name: Name of the stream.
        group_id: Group identifier.
        item_id: Item identifier.
    """

    stream_name: str
    group_id: str
    item_id: str


class StreamSetInput(BaseModel):
    """Input for stream set operation.

    Attributes:
        stream_name: Name of the stream.
        group_id: Group identifier.
        item_id: Item identifier.
        data: Data to store.
    """

    stream_name: str
    group_id: str
    item_id: str
    data: Any


class StreamDeleteInput(BaseModel):
    """Input for stream delete operation.

    Attributes:
        stream_name: Name of the stream.
        group_id: Group identifier.
        item_id: Item identifier.
    """

    stream_name: str
    group_id: str
    item_id: str


class StreamListInput(BaseModel):
    """Input for stream list operation.

    Attributes:
        stream_name: Name of the stream.
        group_id: Group identifier.
    """

    stream_name: str
    group_id: str


class StreamListGroupsInput(BaseModel):
    """Input for stream list groups operation.

    Attributes:
        stream_name: Name of the stream.
    """

    stream_name: str


class StreamUpdateInput(BaseModel):
    """Input for stream update operation.

    Attributes:
        stream_name: Name of the stream.
        group_id: Group identifier.
        item_id: Item identifier.
        ops: Ordered list of update operations to apply atomically.
    """

    stream_name: str
    group_id: str
    item_id: str
    ops: list["UpdateOp"]


class UpdateOpError(BaseModel):
    """Per-op error returned by ``state::update`` / ``stream::update``.

    Emitted by the ``merge`` and ``append`` ops when input violates the
    validation bounds, and by ``append`` for its type-mismatch and
    target-not-object cases. Successfully applied ops are still
    reflected in the response's ``new_value``.

    Attributes:
        op_index: Index of the offending op within the original ``ops`` array.
        code: Stable error code, e.g. ``"merge.path.too_deep"``.
        message: Human-readable description with concrete numbers when applicable.
        doc_url: Optional documentation URL.
    """

    op_index: int
    code: str
    message: str
    doc_url: str | None = None


class StreamSetResult(BaseModel, Generic[TData]):
    """Result of stream set operation.

    Attributes:
        old_value: Previous value (if it existed).
        new_value: New value that was stored.
    """

    old_value: TData | None = None
    new_value: TData


class StreamUpdateResult(BaseModel, Generic[TData]):
    """Result of stream update operation.

    Attributes:
        old_value: Previous value (if it existed).
        new_value: New value after the update.
        errors: Per-op errors. Emitted by ``merge`` and ``append`` for
            validation rejections (path depth/size, value depth, or a
            ``__proto__`` / ``constructor`` / ``prototype`` segment or
            top-level key) and by ``append`` for the
            ``append.type_mismatch`` and ``append.target_not_object``
            surfaces. Successfully applied ops are still reflected in
            ``new_value``. The field is omitted from the JSON wire when empty.
    """

    old_value: TData | None = None
    new_value: TData
    # Per-op errors. Emitted by ``merge`` and ``append`` for validation
    # rejections (path/value bounds, proto-pollution segments) and by
    # ``append`` for the ``append.type_mismatch`` and
    # ``append.target_not_object`` surfaces. Field is omitted from the
    # JSON wire when empty. ``default_factory`` is used (not ``= []``)
    # to keep Pydantic's parameterized-Generic + default handling
    # well-behaved across Python versions.
    errors: list[UpdateOpError] = Field(default_factory=list)


class StreamDeleteResult(BaseModel):
    """Result of stream delete operation.

    Attributes:
        old_value: Previous value (if it existed).
    """

    old_value: Any | None = None


class UpdateSet(BaseModel):
    """Set a field at the given path to a value.

    Attributes:
        path: First-level field path. Use an empty string to target the root value.
        value: Value to set.
    """

    type: str = "set"
    path: str
    value: Any


class UpdateIncrement(BaseModel):
    """Increment a numeric field by a given amount.

    Attributes:
        path: First-level field path.
        by: Amount to increment by.
    """

    type: str = "increment"
    path: str
    by: int | float


class UpdateDecrement(BaseModel):
    """Decrement a numeric field by a given amount.

    Attributes:
        path: First-level field path.
        by: Amount to decrement by.
    """

    type: str = "decrement"
    path: str
    by: int | float


class UpdateAppend(BaseModel):
    """Append an element to an array, concatenate a string, or push at a nested path.

    The target is the root (when ``path`` is omitted, an empty string,
    or an empty list), a single first-level key (when ``path`` is a
    non-empty string), or an arbitrary nested location (when ``path``
    is a list of literal segments).

    Path forms accepted (mirrors :class:`UpdateMerge` after #1547):
      - ``None`` / ``""`` / ``[]``: append at the root.
      - ``"foo"``: append at the first-level key ``foo``. A dotted
        string like ``"a.b"`` is the literal key ``"a.b"``, *not*
        traversed as ``a -> b``.
      - ``["a", "b", "c"]``: nested path; each element is a literal
        segment.

    Engine semantics:
      - Missing/non-object intermediates along a nested path are
        auto-created/replaced with ``{}``.
      - At the leaf:
          - missing/null + nested path -> ``[value]`` (always an array)
          - missing/null + single-string path -> string-as-string for
            the string-concat tier, otherwise ``[value]``
          - existing array -> push
          - existing string + string value -> concatenate
          - existing object/scalar at the leaf -> ``append.type_mismatch``

    Validation: invalid paths (depth > 32 segments, segment > 256
    bytes, or any ``__proto__`` / ``constructor`` / ``prototype``
    segment) are rejected with a structured error in the ``errors``
    field of the ``state::update`` / ``stream::update`` response. The
    append does not apply when an error is returned for that op.

    Attributes:
        path: Optional path to the append target. Accepts a single
            first-level key (legacy string) or a list of literal segments
            for nested append. See :data:`MergePath`.
        value: Value to append. String targets only accept string values.
    """

    type: str = "append"
    # Optional. Accepts a single string (legacy / first-level key) or
    # a list of literal segments (nested append). ``None`` / ``""`` /
    # ``[]`` all route to root append. See :data:`MergePath`.
    path: MergePath | None = None
    value: Any

    @model_serializer(mode="wrap")
    def _omit_none_path(self, handler):  # type: ignore[no-untyped-def]
        # Drop ``path: None`` from the wire so cross-SDK consumers see
        # the field absent rather than ``null``. Mirrors the Rust
        # ``#[serde(skip_serializing_if = "Option::is_none")]`` on
        # ``UpdateOp::Append.path``.
        data = handler(self)
        if data.get("path") is None:
            data.pop("path", None)
        return data


class UpdateRemove(BaseModel):
    """Remove a field at the given path.

    Attributes:
        path: First-level field path.
    """

    type: str = "remove"
    path: str


class UpdateMerge(BaseModel):
    """Shallow merge an object into the target.

    The target is the root (when ``path`` is omitted, an empty string,
    or an empty list) or an arbitrary nested location specified by an
    array of literal segments.

    Path forms accepted:
      - ``None`` / ``""`` / ``[]``: merge at the root.
      - ``"foo"``: equivalent to ``["foo"]``; single first-level key.
      - ``["a", "b", "c"]``: nested path. Each element is a *literal*
        key. ``["a.b"]`` writes a single key named ``"a.b"``, not
        ``a -> b``.

    Engine semantics:
      - Missing or non-object intermediates along the path are
        auto-replaced with ``{}``.
      - The merge is shallow at the target node (top-level keys of
        ``value`` overwrite same-named keys; siblings preserved).

    Validation: invalid paths/values (depth > 32 segments, segment >
    256 bytes, value depth > 16, > 1024 top-level keys, or any
    ``__proto__`` / ``constructor`` / ``prototype`` segment or
    top-level key) are rejected with a structured error in the
    ``errors`` array of the ``state::update`` / ``stream::update``
    response. The merge does not apply when an error is returned.

    Attributes:
        path: Optional path to the merge target. See :data:`MergePath`.
        value: Object to merge. Must be a JSON object.
    """

    type: str = "merge"
    # Optional. Accepts a single string or a list of literal segments.
    # Pydantic resolves ``str | list[str]`` via smart-union: string
    # input -> str, array input -> list[str]. See :data:`MergePath`.
    path: MergePath | None = None
    value: Any

    @model_serializer(mode="wrap")
    def _omit_none_path(self, handler):  # type: ignore[no-untyped-def]
        # Mirrors the same skip-when-none rule applied to
        # ``UpdateOp::Merge.path`` in the Rust SDK so cross-SDK wire
        # payloads are byte-identical for root merges.
        data = handler(self)
        if data.get("path") is None:
            data.pop("path", None)
        return data


UpdateOp = UpdateSet | UpdateIncrement | UpdateDecrement | UpdateAppend | UpdateRemove | UpdateMerge


class StreamTriggerConfig(BaseModel):
    """Trigger config for ``stream`` triggers. Filters which item changes fire the handler.

    Attributes:
        stream_name: Stream name to watch. Only changes on this stream fire the handler.
        group_id: If set, only changes within this group fire the handler.
        item_id: If set, only changes to this specific item fire the handler.
        condition_function_id: Function ID for conditional execution. If it returns ``False``, the handler is skipped.
    """

    stream_name: str
    group_id: str | None = None
    item_id: str | None = None
    condition_function_id: str | None = None


class StreamJoinLeaveTriggerConfig(BaseModel):
    """Trigger config for ``stream:join`` and ``stream:leave`` triggers.

    Attributes:
        condition_function_id: Function ID for conditional execution. If it returns ``False``, the handler is skipped.
    """

    condition_function_id: str | None = None


class StreamChangeEventDetail(BaseModel):
    """Detail of a stream change event containing the mutation type and data.

    Attributes:
        type: The kind of mutation (create, update, or delete).
        data: The data associated with the event.
    """

    type: Literal["create", "update", "delete"]
    data: Any


class StreamChangeEvent(BaseModel):
    """Handler input for ``stream`` triggers, fired when an item changes via ``stream::set``, ``stream::update``, or ``stream::delete``.

    Attributes:
        type: The event type.
        timestamp: Unix timestamp of the event.
        streamName: The stream where the change occurred.
        groupId: The group where the change occurred.
        id: The item ID that changed.
        event: The event detail containing mutation type and data.
    """

    type: Literal["stream"]
    timestamp: int
    streamName: str
    groupId: str
    id: str | None = None
    event: StreamChangeEventDetail
