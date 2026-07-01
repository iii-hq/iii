export interface WhyCard {
  title: string
  body: string
}

export const WHY_CARDS: WhyCard[] = [
  {
    title: 'stringly-typed ids',
    body: 'the function id is a bare string. a typo is a runtime 404, and you find it in production.',
  },
  {
    title: 'hand-mirrored types',
    body: 'you re-type the input and output in every consumer; they drift the moment the worker changes.',
  },
  {
    title: 'a false sense of safety',
    body: 'the sdk trigger<TInput, TOutput> generics are real, but you fill them in by hand, tied to nothing.',
  },
  {
    title: 'no cross-language parity',
    body: 'the same worker gets re-typed separately in typescript, rust, and python.',
  },
  {
    title: 'untyped trigger payloads',
    body: 'binding harness::turn-completed means an any-shaped config and an any-shaped handler.',
  },
  {
    title: 'drift surfaces late',
    body: 'a renamed field still compiles, then breaks at runtime one deploy later.',
  },
]
