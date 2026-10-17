"""Host values seen through JSON (filters.md §3).

The evaluator answers as if the document were a JSON round trip of the host
value, without serializing it: every value is mapped lazily, one step at a
time, when a path reaches it.

Python specifics:

* ``None`` is ``null`` (a value), never absent; an absent root or member is a
  missing key.
* ``bool`` is not a number (``True == 1`` holds in Python, not in JSON).
* Numbers compare as IEEE-754 doubles: an ``int`` beyond 2^53 is rounded to
  the nearest double first, as every other implementation does. A number no
  double can hold (``10**400``, ``Decimal("1e400")``), like a non-finite one,
  is ``null``.
* A dict key is matched by its JSON form, as ``json.dumps`` writes it: ``1``
  is ``"1"``, ``True`` is ``"true"``, ``None`` is ``"null"``. A member under a
  key JSON cannot write (a tuple) is absent.
* ``str`` compares by code point, which is UTF-8 byte order.
* ``Decimal`` is read through its JSON encoding, a number; ``datetime``,
  ``date`` and ``time`` through theirs, the ISO string (pydantic's JSON form,
  ``Z`` for UTC). A pydantic model is read as its JSON dump, and other values
  pydantic can encode (dataclasses, sets, enums, UUIDs) as pydantic encodes
  them. A value JSON cannot encode is absent (``null`` inside an array).
"""

from __future__ import annotations

import math
from datetime import date, datetime, time
from decimal import Decimal
from typing import Any

from pydantic import BaseModel
from pydantic_core import to_jsonable_python


class _Absent:
    """The value of a path that resolves to nothing."""

    __slots__ = ()

    def __repr__(self) -> str:
        return "ABSENT"


ABSENT: Any = _Absent()


def json_view(value: Any) -> Any:
    """One step of JSON encoding: a JSON-shaped value, or :data:`ABSENT`.

    Dicts, lists and tuples are returned as they are; their members are
    mapped when a path steps into them.
    """
    if value is None or isinstance(value, (str, bool, dict, list, tuple)):
        return value
    if isinstance(value, int):
        if value.bit_length() > 1023:
            try:
                float(value)
            except OverflowError:
                return None  # no double holds it
        return value
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if isinstance(value, Decimal):
        if not value.is_finite():
            return None
        number = float(value)
        return number if math.isfinite(number) else None
    if isinstance(value, (datetime, date, time)):
        return to_jsonable_python(value)
    if isinstance(value, BaseModel):
        try:
            return value.model_dump(mode="json", by_alias=True, exclude_none=True)
        except Exception:
            return ABSENT
    try:
        return json_view(to_jsonable_python(value))
    except Exception:
        return ABSENT


def array_element(array: list[Any] | tuple[Any, ...], index: int) -> Any:
    """Element ``index`` as JSON writes it: a member JSON drops is ``null``."""
    v = json_view(array[index])
    return None if v is ABSENT else v


_NON_STRING_FORMS = frozenset({"true", "false", "null", "NaN", "Infinity", "-Infinity"})


def json_key(key: Any) -> str | None:
    """A dict key as ``json.dumps`` writes it; ``None`` when it cannot."""
    if isinstance(key, str):
        return key
    if key is True:
        return "true"
    if key is False:
        return "false"
    if key is None:
        return "null"
    if isinstance(key, int):
        return int.__repr__(key)
    if isinstance(key, float):
        if math.isnan(key):
            return "NaN"
        if math.isinf(key):
            return "Infinity" if key > 0 else "-Infinity"
        return float.__repr__(key)
    return None


def _maybe_from_non_string(key: str) -> bool:
    """Whether a non-string key (a number, a bool, ``None``) could write as ``key``."""
    return key in _NON_STRING_FORMS or (key != "" and key[0] in "-0123456789")


def object_member(obj: dict[Any, Any], key: str) -> Any:
    """Member ``key`` of a dict, by the key's JSON form; a missing key (or a
    member JSON drops) is absent. When keys collide in JSON (``"1"`` and
    ``1``), the one inserted last wins, as a JSON parser reads what
    ``json.dumps`` writes."""
    if not _maybe_from_non_string(key):
        return json_view(obj[key]) if key in obj else ABSENT
    found: Any = ABSENT
    for k, v in obj.items():
        if json_key(k) == key:
            found = v
    return ABSENT if found is ABSENT else json_view(found)


def is_number(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def as_double(n: int | float) -> float:
    """The IEEE-754 double a JSON parser reads ``n`` as."""
    try:
        return float(n)
    except OverflowError:
        return math.inf if n > 0 else -math.inf


def json_equals(host: Any, literal: Any) -> bool:
    """JSON deep equality between a mapped host value and a filter literal.

    Numbers compare as doubles (``1 == 1.0``, ``-0 == 0``), object key order
    is ignored, array order is not, and there is no coercion between types.
    """
    if literal is None:
        return host is None
    if isinstance(literal, bool):
        return isinstance(host, bool) and host is literal
    if isinstance(literal, (int, float)):
        return is_number(host) and as_double(host) == as_double(literal)
    if isinstance(literal, str):
        return isinstance(host, str) and host == literal
    if isinstance(literal, list):
        if not isinstance(host, (list, tuple)) or len(host) != len(literal):
            return False
        for i, item in enumerate(literal):
            if not json_equals(array_element(host, i), item):
                return False
        return True
    if isinstance(literal, dict):
        if not isinstance(host, dict):
            return False
        for k, v in literal.items():
            member = object_member(host, k)
            if member is ABSENT or not json_equals(member, v):
                return False
        # the host must not hold more members than the literal (keys that
        # collide in JSON are one member: the last inserted)
        members: dict[str, Any] = {}
        for k, v in host.items():
            key = json_key(k)
            if key is not None:
                members[key] = v
        present = 0
        for v in members.values():
            if json_view(v) is not ABSENT:
                present += 1
                if present > len(literal):
                    return False
        return present == len(literal)
    return False


def to_json_value(value: Any) -> Any:
    """The eager form of :func:`json_view`: the whole value as JSON reads it
    (members JSON drops are left out, ``null`` inside arrays). :data:`ABSENT`
    when the value itself cannot be encoded."""
    v = json_view(value)
    if isinstance(v, dict):
        out: dict[str, Any] = {}
        for k, member in v.items():
            key = json_key(k)
            if key is None:
                continue
            mapped = to_json_value(member)
            if mapped is ABSENT:
                out.pop(key, None)  # the last colliding key wins, also when JSON drops it
            else:
                out[key] = mapped
        return out
    if isinstance(v, (list, tuple)):
        items = []
        for item in v:
            mapped = to_json_value(item)
            items.append(None if mapped is ABSENT else mapped)
        return items
    return v
