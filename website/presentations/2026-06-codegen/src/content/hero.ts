export interface HeroStat {
  value: string
  label: string
}
export interface HeroClaim {
  title: string
  body: string
}

export const HERO_STATS: HeroStat[] = [
  { value: '4', label: 'languages' },
  { value: '1', label: 'config file' },
  { value: '3', label: 'generation modes' },
  { value: '0', label: 'types hand-written' },
]

export const HERO_CLAIMS: HeroClaim[] = [
  {
    title: 'simplicity',
    body: 'one command turns a codegen.yml into every type and wrapper you need to call a worker.',
  },
  {
    title: 'structure',
    body: 'types come from the engine catalog: the same json schema each worker already registered.',
  },
  {
    title: 'correctness',
    body: 'a typo in a function id, or a renamed field, becomes a compile error instead of a production 404.',
  },
]
