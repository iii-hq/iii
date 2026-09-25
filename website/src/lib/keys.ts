// React keys for lists whose items carry no id of their own.

/**
 * Pairs each item with a key built from its own text, disambiguated when the
 * same text appears more than once ("foo", "foo·2", …). For static or
 * append-only content this identifies an item by what it is, not where it
 * sits, so React never has to guess when a list changes.
 */
export function keyed<T>(items: readonly T[], text: (item: T) => string): { key: string; item: T }[] {
  const seen = new Map<string, number>()
  return items.map((item) => {
    const base = text(item)
    const n = (seen.get(base) ?? 0) + 1
    seen.set(base, n)
    return { key: n === 1 ? base : `${base}·${n}`, item }
  })
}

const ids = new WeakMap<object, string>()
let next = 0

/** A stable key for an object by identity (a script line, a row's source), the same for the object's lifetime. */
export function identityKey(o: object): string {
  let id = ids.get(o)
  if (!id) {
    id = `k${next++}`
    ids.set(o, id)
  }
  return id
}
