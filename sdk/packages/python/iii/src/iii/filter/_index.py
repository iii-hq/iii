"""Many filters matched against one document by an index of their equality
conditions (decision 55), the rule of ``iii_filter::FilterIndex``.

A filter matches a document only if every clause of its top-level ``$and``
does. A clause on a path with ``$eq`` of a scalar (or ``$in`` of scalars)
names the values the document's value at that path must equal. The index
files each filter under one such condition, path to value to filters, and
keeps the filters without one in a list it scans. Matching a document
resolves each indexed path once, looks its value up, and evaluates the whole
filter of only the entries found there and of the scanned ones.

The index rules out only entries whose condition the document fails, and
evaluates every other entry's filter in full, so its matches are exactly the
entries a scan of every filter yields, in the order the entries were given.

Values are keyed by the equality of the filter language as this SDK's
evaluator applies it (``_value.json_equals``): numbers by their value as an
IEEE-754 double (``1`` and ``1.0`` are one value, ``-0`` is ``0``), strings
by their code points, ``null`` and booleans as themselves, and a value of
one type never equals a value of another. An array or object operand is
never indexed: its clause is evaluated as part of the filter. A host value
whose equality is its own (a ``str`` subclass with its own ``__eq__``, a
number whose ``__float__`` returns something other than a ``float``) rules
nothing out: every entry filed under its path is evaluated.
"""

from __future__ import annotations

from collections.abc import Callable, Hashable, Iterable, Mapping, Sequence
from typing import TYPE_CHECKING, Any, Generic, TypeVar

from ._path import Path, Resolver
from ._value import ABSENT, as_double

if TYPE_CHECKING:
    from . import CompiledFilter

K = TypeVar("K", bound=Hashable)

#: The key of one scalar: ``("s", text)``, ``("n", double)``, ``("b", flag)``
#: or ``("z", None)``. The tag keeps ``1.0``, ``True`` and ``"1"`` apart,
#: which Python's own equality would not.
ScalarKey = tuple[str, Any]

#: A path as the index keys it: its root and the keys of its segments (what
#: its resolver reads, whatever the escapes it was written with).
PathKey = tuple[str, tuple[str, ...]]

#: One equality condition of a filter's top-level ``$and``: its path, the
#: resolver its clause reads the path with, and the keys of the values the
#: document's value must equal one of.
Term = tuple[PathKey, Resolver, frozenset[ScalarKey]]


def path_key(path: Path) -> PathKey:
    return (path.root, tuple(segment.key for segment in path.segments))


_NULL: ScalarKey = ("z", None)


class _Probe:
    """What a document holds at an indexed path, when it is not a scalar key."""

    __slots__ = ("name",)

    def __init__(self, name: str) -> None:
        self.name = name

    def __repr__(self) -> str:
        return self.name


#: An array, an object, an absent path, or a value whose equality fails:
#: it equals no indexed scalar, so every entry filed under the path is ruled out.
NO_SCALAR = _Probe("NO_SCALAR")
#: A value the index cannot key: no entry filed under the path is ruled out.
UNKNOWN = _Probe("UNKNOWN")


def literal_key(literal: Any) -> ScalarKey | None:
    """The key of a filter literal (a JSON value as ``json.loads`` reads it);
    ``None`` for an array or an object."""
    if literal is None:
        return _NULL
    if isinstance(literal, bool):
        return ("b", literal)
    if isinstance(literal, (int, float)):
        return ("n", as_double(literal))
    if isinstance(literal, str):
        return ("s", literal)
    return None


def probe(value: Any) -> ScalarKey | _Probe:
    """The key a resolved document value is looked up by: the key of the
    literals it equals, :data:`NO_SCALAR` for one that equals no scalar
    literal, or :data:`UNKNOWN`.

    ``value`` is what the filter's path resolver returns, already seen
    through JSON (``_value.json_view``): ``None``, a ``bool``, an ``int`` or
    ``float`` (subclasses too), a ``str`` (subclasses too), a ``dict``,
    ``list`` or ``tuple``, or :data:`ABSENT`.
    """
    kind = type(value)
    if kind is str:
        return ("s", value)
    if kind is float:
        return ("n", value)
    if kind is int:
        return ("n", as_double(value))
    if kind is bool:
        return ("b", value)
    if value is None:
        return _NULL
    if value is ABSENT or isinstance(value, (dict, list, tuple)):
        return NO_SCALAR
    if isinstance(value, str):
        # `host == literal` with the literal a plain `str`: the subclass's own `__eq__` runs first.
        eq: object = kind.__eq__
        if eq is str.__eq__:
            return ("s", str.__str__(value))
        return UNKNOWN
    if isinstance(value, (int, float)):
        # The evaluator compares `as_double(host)` with the literal's double.
        try:
            number = as_double(value)
        except Exception:
            return NO_SCALAR  # the evaluator's comparison fails too: the clause is no match
        return ("n", number) if type(number) is float else UNKNOWN
    return UNKNOWN


class _Entry(Generic[K]):
    """One filter as the index holds it: its position, its key (``key``), the
    filter (``filter``), and the condition it is filed under (``None``:
    scanned)."""

    __slots__ = ("filter", "key", "path", "seq", "values")

    def __init__(
        self, seq: int, key: K, filter_: CompiledFilter, path: PathKey | None, values: frozenset[ScalarKey]
    ) -> None:
        self.seq = seq
        self.key = key
        self.filter = filter_
        self.path = path
        self.values = values


class _PathIndex(Generic[K]):
    """The entries filed under one path, by value."""

    __slots__ = ("buckets", "entries", "resolve")

    def __init__(self, resolve: Resolver) -> None:
        self.resolve = resolve
        self.buckets: dict[ScalarKey, list[_Entry[K]]] = {}
        #: Every entry filed under the path, in order (for an :data:`UNKNOWN` value).
        self.entries: list[_Entry[K]] = []

    def probe(self, doc: Mapping[str, Any]) -> ScalarKey | _Probe:
        try:
            return probe(self.resolve(doc, None))
        except Exception:
            # The evaluator's own read of the path fails the same way: its clause is no match.
            return NO_SCALAR

    def lookup(self, doc: Mapping[str, Any]) -> Sequence[_Entry[K]]:
        """The entries ``doc`` does not rule out."""
        found = self.probe(doc)
        if isinstance(found, _Probe):
            return self.entries if found is UNKNOWN else ()
        return self.buckets.get(found, ())


def _seq(entry: _Entry[Any]) -> int:
    return entry.seq


class FilterIndex(Generic[K]):
    """Filters, each with a key, matched against a document by an index of
    their equality conditions (see the module docs). Built once from the
    entries in order; never changed afterwards, so any thread may read it.

    :meth:`matches` yields exactly the keys whose filter matches the
    document, in the order the entries were given: what evaluating every
    entry's filter in that order yields.
    """

    __slots__ = ("_filed", "_paths", "_scanned")

    def __init__(self, entries: Iterable[tuple[K, CompiledFilter]]) -> None:
        paths: dict[PathKey, _PathIndex[K]] = {}
        scanned: list[_Entry[K]] = []
        filed: dict[K, _Entry[K]] = {}
        for seq, (key, filter_) in enumerate(entries):
            if key in filed:
                raise ValueError(f"FilterIndex: the key {key!r} is given twice")
            chosen = _condition(filter_, paths)
            if chosen is None:
                entry: _Entry[K] = _Entry(seq, key, filter_, None, frozenset())
                scanned.append(entry)
            else:
                path, resolve, values = chosen
                entry = _Entry(seq, key, filter_, path, values)
                index = paths.get(path)
                if index is None:
                    index = paths[path] = _PathIndex(resolve)
                index.entries.append(entry)
                for value in values:
                    index.buckets.setdefault(value, []).append(entry)
            filed[key] = entry
        self._filed = filed
        self._paths = paths
        self._scanned = scanned

    def __len__(self) -> int:
        return len(self._filed)

    def __contains__(self, key: object) -> bool:
        return key in self._filed

    def keys(self) -> list[K]:
        """Every key, in order."""
        return [entry.key for entry in sorted(self._filed.values(), key=_seq)]

    def candidate_entries(self, doc: Mapping[str, Any]) -> Sequence[_Entry[K]]:
        """The entries ``doc`` does not rule out, in order: those filed under
        its values of the indexed paths, and the scanned ones. The keys whose
        filter matches ``doc`` are exactly those of the entries here whose
        ``filter`` matches it."""
        if not self._paths:
            return self._scanned
        found: list[_Entry[K]] = []
        lists = 0
        for index in self._paths.values():
            bucket = index.lookup(doc)
            if bucket:
                found.extend(bucket)
                lists += 1
        if self._scanned:
            found.extend(self._scanned)
            lists += 1
        if lists > 1:
            found.sort(key=_seq)
        return found

    def candidates(self, doc: Mapping[str, Any]) -> int:
        """How many filters a match of ``doc`` evaluates."""
        return len(self.candidate_entries(doc))

    def matches(self, doc: Mapping[str, Any], accept: Callable[[K], bool] | None = None) -> list[K]:
        """The keys whose filter matches ``doc`` (and that ``accept``, when
        given, accepts; it runs before the filter), in order."""
        out: list[K] = []
        for entry in self.candidate_entries(doc):
            if (accept is None or accept(entry.key)) and entry.filter.matches(doc):
                out.append(entry.key)
        return out

    def select(self, keys: Iterable[K], doc: Mapping[str, Any], accept: Callable[[K], bool] | None = None) -> list[K]:
        """Those of ``keys`` (in their order) whose filter matches ``doc`` and
        that ``accept`` accepts: each path is resolved once, and an entry is
        evaluated only when ``doc`` does not rule it out. A key the index
        does not hold is left out."""
        out: list[K] = []
        seen: dict[PathKey, ScalarKey | _Probe] = {}
        for key in keys:
            if accept is not None and not accept(key):
                continue
            entry = self._filed.get(key)
            if entry is None:
                continue
            path = entry.path
            if path is not None:
                found = seen.get(path)
                if found is None:
                    found = seen[path] = self._paths[path].probe(doc)
                if isinstance(found, _Probe):
                    if found is NO_SCALAR:
                        continue
                elif found not in entry.values:
                    continue
            if entry.filter.matches(doc):
                out.append(key)
        return out


def _condition(filter_: CompiledFilter, paths: Mapping[PathKey, _PathIndex[Any]]) -> Term | None:
    """The condition ``filter_`` is filed under: the equality condition of
    its top-level ``$and`` that the fewest entries share now (an ``$eq``
    before an ``$in``, then the fewest values); ``None`` (the scanned list)
    when it has none an index serves."""
    terms = filter_.equality_terms
    if len(terms) <= 1:
        return terms[0] if terms else None
    best: Term | None = None
    best_score: tuple[bool, int, int] | None = None
    for term in terms:
        path, _resolve, values = term
        index = paths.get(path)
        sharing = 0
        if index is not None:
            for value in values:
                bucket = index.buckets.get(value)
                if bucket:
                    sharing += len(bucket)
        score = (len(values) > 1, sharing, len(values))
        if best_score is None or score < best_score:
            best, best_score = term, score
    return best
