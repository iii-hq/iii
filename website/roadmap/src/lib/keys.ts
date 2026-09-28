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
