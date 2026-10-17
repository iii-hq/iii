"""Validation (filters.md §2, §4) and compilation into closures, in one pass.

Mirrors ``crates/iii-filter`` and the Node SDK evaluator: the same limits,
the same error code (``INVALID_FILTER``) and the same semantics, so a filter
evaluated here, in the engine or in another SDK answers the same. The filter
is read through JSON first (compact, non-ASCII unescaped: the encoding the
size limit is measured on), so what is validated is exactly what travels on
the wire.

Limits, as the vectors define them:

* depth: the top-level filter is at depth 1; the filters of a ``$or``, the
  operand of ``$not`` and the operand of ``$any`` are one level deeper than
  the object holding them; the filters of a ``$and`` stay at the same level;
* nodes: every operator (``$eq`` ... ``$any``) and every ``$or`` / ``$not``
  counts one; paths and ``$and`` are free;
* encoded size: the compact JSON of the filter as given, in UTF-8 bytes.
"""

from __future__ import annotations

import json
import math
from collections.abc import Callable, Mapping
from typing import Any

from ._errors import (
    MAX_DEPTH,
    MAX_ENCODED_BYTES,
    MAX_JSON_DEPTH,
    MAX_LITERAL_DEPTH,
    MAX_NODES,
    MAX_SAFE_LITERAL,
    MAX_SET_ITEMS,
    FilterError,
)
from ._path import Resolver, compile_path, parse_path
from ._value import ABSENT, array_element, as_double, is_number, json_equals

#: A compiled filter node: ``element`` is the current ``$any`` element (``@``).
Pred = Callable[[Mapping[str, Any], Any], bool]
#: A value operator: ``value`` is :data:`ABSENT` when the path is absent.
OpFn = Callable[[Any, Mapping[str, Any]], bool]

VALUE_OPERATORS = frozenset({"$eq", "$ne", "$in", "$nin", "$gt", "$gte", "$lt", "$lte", "$exists", "$prefix", "$any"})


class _Budget:
    __slots__ = ("used",)

    def __init__(self) -> None:
        self.used = 0

    def take(self) -> None:
        self.used += 1
        if self.used > MAX_NODES:
            raise FilterError.invalid(f"more than {MAX_NODES} operator / clause nodes")


def _kind(v: Any) -> str:
    if v is None:
        return "null"
    if isinstance(v, list):
        return "an array"
    if isinstance(v, bool):
        return "a boolean"
    if isinstance(v, (int, float)):
        return "a number"
    if isinstance(v, str):
        return "a string"
    return "an object"


def _always(_doc: Mapping[str, Any], _el: Any) -> bool:
    return True


def _where(at: str) -> str:
    return "the filter" if at == "" else f"`{at}`"


def _check_string(s: str, at: str, what: str) -> None:
    """A string serde_json would refuse (a lone surrogate) is refused here too."""
    try:
        s.encode("utf-8")
    except UnicodeEncodeError:
        raise FilterError.invalid(
            f"{_where(at)}: {what} holds a lone UTF-16 surrogate, which is not valid Unicode"
        ) from None


def check_json(src: Any) -> None:
    """Checks that ``src`` is JSON as it stands: dicts with ``str`` keys,
    lists (or tuples), strings, finite numbers within ±2^53, booleans and
    ``None``. Anything JSON would not carry as it is (a ``Decimal``, a
    ``datetime``, a set, another mapping, NaN) is ``INVALID_FILTER``, naming
    where it is: coercing it could turn a filter into one that matches more.
    The whole value nests at most MAX_JSON_DEPTH arrays / objects. Iterative,
    and bounded by the encoded size limit (every JSON value takes at least one
    byte).
    """
    stack: list[tuple[Any, str, int]] = [(src, "", 1)]
    visited = 0
    while stack:
        value, at, level = stack.pop()
        visited += 1
        if visited > MAX_ENCODED_BYTES:
            raise FilterError.invalid(f"the filter encodes to more than {MAX_ENCODED_BYTES} bytes")
        if value is None or isinstance(value, bool):
            continue
        if isinstance(value, str):
            _check_string(value, at, "a string")
            continue
        if isinstance(value, (int, float)):
            if isinstance(value, float) and not math.isfinite(value):
                raise FilterError.invalid(f"{_where(at)}: {value} is not a JSON number")
            # |n| <= 2^53, so every implementation reads every literal alike.
            if not abs(value) <= MAX_SAFE_LITERAL:
                raise FilterError.invalid(f"{_where(at)}: the number {value} is outside ±2^53")
            continue
        if isinstance(value, (list, tuple, dict)) and level > MAX_JSON_DEPTH:
            raise FilterError.invalid(f"the filter nests more than {MAX_JSON_DEPTH} arrays / objects")
        if isinstance(value, (list, tuple)):
            for i, item in enumerate(value):
                stack.append((item, f"{at}[{i}]", level + 1))
            continue
        if isinstance(value, dict):
            for key, member in value.items():
                if not isinstance(key, str):
                    raise FilterError.invalid(f"{_where(at)}: the key {key!r} is not a string")
                path = key if at == "" else f"{at}.{key}"
                _check_string(key, path, "the key")
                stack.append((member, path, level + 1))
            continue
        raise FilterError.invalid(f"{_where(at)}: a {type(value).__name__} is not JSON")


def compile_source(src: Any, trusted: bool = False) -> tuple[Pred, frozenset[str], dict[str, Any], bool]:
    """Validates ``src`` and compiles it.

    Returns ``(pred, roots, json, match_all)``. Raises :class:`FilterError`
    (``INVALID_FILTER``) when the filter is not valid. ``trusted`` skips the
    encoded size limit, for filters the engine already validated (the normal
    form it pushes to owners, which can be longer than the filter written).
    """
    if not isinstance(src, dict):
        raise FilterError.invalid(f"a filter must be a JSON object, found {_kind(src)}")
    check_json(src)
    text = json.dumps(src, separators=(",", ":"), ensure_ascii=False)
    if not trusted and len(text.encode("utf-8")) > MAX_ENCODED_BYTES:
        raise FilterError.invalid(f"the filter encodes to more than {MAX_ENCODED_BYTES} bytes")
    source: dict[str, Any] = json.loads(text)
    roots: set[str] = set()
    clauses: list[Pred] = []
    _collect_clauses(source, 1, False, _Budget(), roots, clauses)
    return _and(clauses), frozenset(roots), source, not clauses


def _parse_filter(src: Any, depth: int, in_any: bool, budget: _Budget, roots: set[str]) -> Pred:
    clauses: list[Pred] = []
    _collect_clauses(src, depth, in_any, budget, roots, clauses)
    return _and(clauses)


def _and(clauses: list[Pred]) -> Pred:
    if not clauses:
        return _always
    if len(clauses) == 1:
        return clauses[0]
    if len(clauses) == 2:
        a, b = clauses

        def both(doc: Mapping[str, Any], el: Any) -> bool:
            return a(doc, el) and b(doc, el)

        return both

    items = tuple(clauses)

    def every(doc: Mapping[str, Any], el: Any) -> bool:
        for clause in items:
            if not clause(doc, el):
                return False
        return True

    return every


def _collect_clauses(src: Any, depth: int, in_any: bool, budget: _Budget, roots: set[str], out: list[Pred]) -> None:
    """Collects the clauses of ``src`` and of every filter its ``$and``s hold.
    ``$and`` adds no depth, so it is walked with an explicit stack."""
    if depth > MAX_DEPTH:
        raise FilterError.invalid(f"filters nested deeper than {MAX_DEPTH} levels")
    pending: list[Any] = [src]
    while pending:
        current = pending.pop()
        if not isinstance(current, dict):
            raise FilterError.invalid(f"a filter must be a JSON object, found {_kind(current)}")
        for key, value in current.items():
            if key == "$and":
                items = _filter_list("$and", value)
                pending.extend(reversed(items))
            elif key == "$or":
                budget.take()
                children = tuple(
                    _parse_filter(item, depth + 1, in_any, budget, roots) for item in _filter_list("$or", value)
                )
                out.append(_any_of(children))
            elif key == "$not":
                budget.take()
                if not isinstance(value, dict):
                    raise FilterError.invalid(f"`$not` takes a filter object, found {_kind(value)}")
                out.append(_negate(_parse_filter(value, depth + 1, in_any, budget, roots)))
            elif key in VALUE_OPERATORS:
                raise FilterError.invalid(f'`{key}` needs a path: write {{"<path>": {{"{key}": ...}}}}')
            elif key.startswith("$"):
                raise FilterError.invalid(f"unknown operator `{key}` (a filter holds paths, `$and`, `$or` and `$not`)")
            else:
                path = parse_path(key, in_any)
                if path.root != "@":
                    roots.add(path.root)
                ops = _parse_operator_object(key, value, depth, budget)
                out.append(_path_clause(compile_path(path), ops))


def _any_of(children: tuple[Pred, ...]) -> Pred:
    def pred(doc: Mapping[str, Any], el: Any) -> bool:
        for child in children:
            if child(doc, el):
                return True
        return False

    return pred


def _negate(child: Pred) -> Pred:
    return lambda doc, el: not child(doc, el)


def _path_clause(resolve: Resolver, ops: list[OpFn]) -> Pred:
    if len(ops) == 1:
        op = ops[0]
        return lambda doc, el: op(resolve(doc, el), doc)
    items = tuple(ops)

    def pred(doc: Mapping[str, Any], el: Any) -> bool:
        value = resolve(doc, el)
        for op in items:
            if not op(value, doc):
                return False
        return True

    return pred


def _filter_list(name: str, value: Any) -> list[Any]:
    if not isinstance(value, list):
        raise FilterError.invalid(f"`{name}` takes an array of filters, found {_kind(value)}")
    if not value:
        raise FilterError.invalid(f"`{name}` takes a non-empty array of filters")
    return value


def _parse_operator_object(path: str, value: Any, depth: int, budget: _Budget) -> list[OpFn]:
    def shorthand() -> FilterError:
        return FilterError.invalid(
            f'`{path}`: expected an operator object such as {{"$eq": ...}}; literal values are not allowed'
        )

    if not isinstance(value, dict):
        raise shorthand()
    if not value:
        raise FilterError.invalid(f"`{path}`: empty operator object")
    if any(not k.startswith("$") for k in value):
        raise shorthand()
    return _parse_operators(path, value, depth, budget)


def _parse_operators(path: str, ops: dict[str, Any], depth: int, budget: _Budget) -> list[OpFn]:
    """The operators of an operator object; ``depth`` is the level of the object holding them."""
    out: list[OpFn] = []
    for name, operand in ops.items():
        budget.take()
        out.append(_parse_operator(path, name, operand, depth, budget))
    return out


def _compare(name: str) -> Callable[[int], bool]:
    if name == "$gt":
        return lambda c: c > 0
    if name == "$gte":
        return lambda c: c >= 0
    if name == "$lt":
        return lambda c: c < 0
    return lambda c: c <= 0


def _parse_operator(path: str, name: str, operand: Any, depth: int, budget: _Budget) -> OpFn:
    def wrong(expected: str) -> FilterError:
        return FilterError.invalid(f"`{path}`: `{name}` takes {expected}, found {_kind(operand)}")

    if name == "$eq":
        _check_literal_depth(path, name, operand)
        return lambda v, _doc: v is not ABSENT and json_equals(v, operand)
    if name == "$ne":
        _check_literal_depth(path, name, operand)
        return lambda v, _doc: v is ABSENT or not json_equals(v, operand)
    if name in ("$in", "$nin"):
        if not isinstance(operand, list):
            raise wrong("an array")
        if len(operand) > MAX_SET_ITEMS:
            raise FilterError.invalid(f"`{path}`: `{name}` has {len(operand)} items (limit {MAX_SET_ITEMS})")
        for item in operand:
            _check_literal_depth(path, name, item)
        items = tuple(operand)

        def member(v: Any) -> bool:
            for item in items:
                if json_equals(v, item):
                    return True
            return False

        if name == "$in":
            return lambda v, _doc: v is not ABSENT and member(v)
        return lambda v, _doc: v is ABSENT or not member(v)
    if name in ("$gt", "$gte", "$lt", "$lte"):
        accept = _compare(name)
        if is_number(operand):
            bound = as_double(operand)

            def number_order(v: Any, _doc: Mapping[str, Any]) -> bool:
                if not is_number(v):
                    return False
                x = as_double(v)
                return accept(-1 if x < bound else 1 if x > bound else 0)

            return number_order
        if isinstance(operand, str):
            text = operand

            def string_order(v: Any, _doc: Mapping[str, Any]) -> bool:
                # Python orders `str` by code point: UTF-8 byte order.
                if not isinstance(v, str):
                    return False
                return accept(-1 if v < text else 1 if v > text else 0)

            return string_order
        raise wrong("a number or a string")
    if name == "$exists":
        if not isinstance(operand, bool):
            raise wrong("a boolean")
        expected = operand
        return lambda v, _doc: (v is not ABSENT) is expected
    if name == "$prefix":
        if not isinstance(operand, str):
            raise wrong("a string")
        prefix = operand
        return lambda v, _doc: isinstance(v, str) and v.startswith(prefix)
    if name == "$any":
        return _parse_any(path, operand, depth, budget)
    if name in ("$and", "$or", "$not"):
        raise FilterError.invalid(f"`{path}`: `{name}` combines filters and is not a value operator")
    raise FilterError.invalid(f"`{path}`: unknown operator `{name}`")


def _parse_any(path: str, operand: Any, depth: int, budget: _Budget) -> OpFn:
    """``$any`` takes an operator object (all keys are value operators) or a
    filter over the element (``@`` paths, ``$and``, ``$or``, ``$not``). ``{}``
    is the empty filter: it matches any non-empty array."""
    if not isinstance(operand, dict):
        raise FilterError.invalid(
            f"`{path}`: `$any` takes an operator object or a filter over `@`, found {_kind(operand)}"
        )
    if depth + 1 > MAX_DEPTH:
        raise FilterError.invalid(f"filters nested deeper than {MAX_DEPTH} levels")
    if operand and all(k in VALUE_OPERATORS for k in operand):
        ops = tuple(_parse_operators(path, operand, depth + 1, budget))

        def any_element(v: Any, doc: Mapping[str, Any]) -> bool:
            if not isinstance(v, (list, tuple)):
                return False
            for i in range(len(v)):
                item = array_element(v, i)
                if all(op(item, doc) for op in ops):
                    return True
            return False

        return any_element
    # `@` paths add no root: the `$any` clause's own path names it.
    node = _parse_filter(operand, depth + 1, True, budget, set())

    def any_match(v: Any, doc: Mapping[str, Any]) -> bool:
        if not isinstance(v, (list, tuple)):
            return False
        for i in range(len(v)):
            if node(doc, array_element(v, i)):
                return True
        return False

    return any_match


def _check_literal_depth(path: str, name: str, literal: Any) -> None:
    """A literal (an ``$eq`` / ``$ne`` operand, an ``$in`` / ``$nin`` item)
    nests at most MAX_LITERAL_DEPTH arrays / objects: a scalar is 0, ``[]``
    and ``[1]`` are 1. It also bounds the recursion of equality."""
    stack: list[tuple[Any, int]] = [(literal, 1)]
    while stack:
        value, level = stack.pop()
        if isinstance(value, dict):
            members: Any = value.values()
        elif isinstance(value, list):
            members = value
        else:
            continue
        if level > MAX_LITERAL_DEPTH:
            raise FilterError.invalid(
                f"`{path}`: `{name}` nests a literal deeper than {MAX_LITERAL_DEPTH} arrays / objects"
            )
        for member in members:
            stack.append((member, level + 1))
