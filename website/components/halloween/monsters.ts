/**
 * Pixel monsters for the haunted iii mark. Every shape is drawn on a 16×16 head grid; accessories may reach 4 cells
 * past it on any side (the face renders a -4..20 viewBox). A rect is [x, y, width, height, fill?].
 */
export type Px = readonly [x: number, y: number, w: number, h: number, fill?: string]

export interface Monster {
  name: string
  /** Skin colour; the front face tints toward it while the bars fold into a square. */
  color: string
  /** Head silhouette, filled with `color`. */
  head: readonly Px[]
  /** Markings painted on the head under the visor. */
  marks?: readonly Px[]
  /** The dark screen the eyes sit on. */
  visor?: readonly Px[]
  /** These blink. */
  eyes: readonly Px[]
  /** Painted over the visor, unanimated. */
  details?: readonly Px[]
  /** Pop out once the face has turned toward the viewer. */
  extras?: readonly Px[]
}

const SQUARE: readonly Px[] = [[0, 0, 16, 16]]
const VISOR: readonly Px[] = [[3, 5, 10, 8, '#0a0a0a']]
const EYES = (fill = '#ffffff'): readonly Px[] => [
  [5, 7, 2, 3, fill],
  [9, 7, 2, 3, fill],
]

export const MONSTERS: readonly Monster[] = [
  {
    name: 'Frankenstein',
    color: '#6fbf4b',
    head: SQUARE,
    marks: [
      [0, 0, 16, 3, '#3a6f27'],
      [1, 3, 3, 1, '#3a6f27'],
      [7, 3, 3, 1, '#3a6f27'],
      [12, 3, 3, 1, '#3a6f27'],
      [4, 14, 8, 1, '#3a6f27'],
      [5, 13, 1, 3, '#3a6f27'],
      [8, 13, 1, 3, '#3a6f27'],
      [10, 13, 1, 3, '#3a6f27'],
    ],
    visor: VISOR,
    eyes: EYES(),
    extras: [
      [-3, 8, 3, 2, '#9a9a9a'],
      [-4, 7, 1, 4, '#d4d4d4'],
      [16, 8, 3, 2, '#9a9a9a'],
      [19, 7, 1, 4, '#d4d4d4'],
    ],
  },
  {
    name: 'Ghost',
    color: '#f1f1f1',
    head: [
      [2, 0, 12, 1],
      [1, 1, 14, 1],
      [0, 2, 16, 14],
    ],
    eyes: [
      [4, 5, 3, 4, '#0a0a0a'],
      [9, 5, 3, 4, '#0a0a0a'],
    ],
    details: [
      [7, 11, 2, 1, '#0a0a0a'],
      [6, 12, 1, 1, '#0a0a0a'],
      [9, 12, 1, 1, '#0a0a0a'],
      [7, 13, 2, 1, '#0a0a0a'],
    ],
    extras: [
      [0, 16, 2, 2, '#f1f1f1'],
      [4, 16, 3, 2, '#f1f1f1'],
      [9, 16, 3, 2, '#f1f1f1'],
      [14, 16, 2, 2, '#f1f1f1'],
      [5, 18, 1, 1, '#f1f1f1'],
      [10, 18, 1, 1, '#f1f1f1'],
    ],
  },
  {
    name: 'Devil',
    color: '#e5484d',
    head: SQUARE,
    marks: [
      [6, 14, 4, 2, '#8f1d22'],
      [7, 13, 2, 1, '#8f1d22'],
    ],
    visor: VISOR,
    eyes: EYES('#ffd60a'),
    details: [
      [4, 6, 3, 1, '#e5484d'],
      [9, 6, 3, 1, '#e5484d'],
      [6, 11, 4, 1, '#ffffff'],
      [6, 12, 1, 1, '#ffffff'],
      [9, 12, 1, 1, '#ffffff'],
    ],
    extras: [
      [1, -2, 3, 2, '#8f1d22'],
      [0, -4, 2, 2, '#8f1d22'],
      [12, -2, 3, 2, '#8f1d22'],
      [14, -4, 2, 2, '#8f1d22'],
    ],
  },
  {
    name: 'Bat',
    color: '#71717f',
    head: SQUARE,
    marks: [[6, 1, 4, 2, '#45454f']],
    visor: VISOR,
    eyes: EYES('#ff5c5c'),
    details: [
      [6, 11, 1, 2, '#ffffff'],
      [9, 11, 1, 2, '#ffffff'],
    ],
    extras: [
      [0, -3, 4, 3, '#71717f'],
      [0, -4, 2, 1, '#71717f'],
      [12, -3, 4, 3, '#71717f'],
      [14, -4, 2, 1, '#71717f'],
      [-4, 4, 4, 2, '#45454f'],
      [-3, 6, 3, 2, '#45454f'],
      [-2, 8, 2, 2, '#45454f'],
      [16, 4, 4, 2, '#45454f'],
      [16, 6, 3, 2, '#45454f'],
      [16, 8, 2, 2, '#45454f'],
    ],
  },
  {
    name: 'Mummy',
    color: '#e8e0cc',
    head: SQUARE,
    marks: [
      [0, 1, 11, 1, '#b3a88c'],
      [5, 3, 11, 1, '#b3a88c'],
      [0, 14, 9, 1, '#b3a88c'],
      [7, 15, 9, 1, '#b3a88c'],
    ],
    visor: [[2, 6, 12, 5, '#0a0a0a']],
    eyes: [
      [4, 7, 2, 3, '#ffffff'],
      [10, 7, 2, 3, '#ffffff'],
    ],
    details: [[7, 6, 2, 5, '#e8e0cc']],
    extras: [
      [16, 12, 2, 1, '#e8e0cc'],
      [17, 13, 1, 3, '#e8e0cc'],
      [18, 15, 1, 2, '#e8e0cc'],
    ],
  },
  {
    name: 'Pumpkin',
    color: '#f59e0b',
    head: [
      [1, 0, 14, 16],
      [0, 1, 16, 14],
    ],
    marks: [
      [5, 0, 1, 16, '#b45309'],
      [10, 0, 1, 16, '#b45309'],
    ],
    visor: VISOR,
    eyes: EYES('#ffd60a'),
    details: [
      [5, 11, 6, 1, '#ffd60a'],
      [6, 11, 1, 1, '#0a0a0a'],
      [9, 11, 1, 1, '#0a0a0a'],
    ],
    extras: [
      [7, -3, 2, 3, '#4d7c0f'],
      [9, -3, 3, 1, '#65a30d'],
      [11, -2, 2, 1, '#65a30d'],
    ],
  },
  {
    name: 'Skull',
    color: '#ebe3d0',
    head: [
      [1, 0, 14, 12],
      [0, 1, 16, 10],
      [3, 12, 10, 4],
    ],
    visor: [
      [2, 4, 5, 5, '#0a0a0a'],
      [9, 4, 5, 5, '#0a0a0a'],
    ],
    eyes: [
      [4, 6, 1, 2, '#ff5c5c'],
      [11, 6, 1, 2, '#ff5c5c'],
    ],
    details: [
      [7, 9, 2, 2, '#0a0a0a'],
      [4, 12, 8, 1, '#0a0a0a'],
      [6, 13, 1, 2, '#0a0a0a'],
      [9, 13, 1, 2, '#0a0a0a'],
      [13, 1, 1, 2, '#a39b86'],
      [12, 3, 1, 1, '#a39b86'],
    ],
  },
  {
    name: 'Zombie',
    color: '#8aab6e',
    head: [
      [0, 0, 12, 16],
      [12, 3, 4, 13],
    ],
    marks: [
      [12, 2, 2, 1, '#8aab6e'],
      [2, 2, 7, 1, '#4f6b3c'],
      [3, 1, 1, 3, '#4f6b3c'],
      [5, 1, 1, 3, '#4f6b3c'],
      [7, 1, 1, 3, '#4f6b3c'],
      [11, 14, 3, 2, '#4f6b3c'],
    ],
    visor: VISOR,
    eyes: [
      [5, 7, 2, 3, '#ffffff'],
      [9, 9, 2, 2, '#ffffff'],
    ],
    details: [
      [5, 11, 3, 1, '#ffffff'],
      [8, 12, 2, 1, '#ffffff'],
    ],
    extras: [
      [3, 16, 1, 2, '#4f6b3c'],
      [3, 18, 1, 1, '#8aab6e'],
    ],
  },
  {
    name: 'Vampire',
    color: '#c4b5fd',
    head: SQUARE,
    marks: [
      [0, 0, 16, 3, '#2e1f4f'],
      [5, 3, 6, 1, '#2e1f4f'],
      [7, 4, 2, 1, '#2e1f4f'],
    ],
    visor: VISOR,
    eyes: EYES('#ff5c5c'),
    details: [
      [6, 11, 1, 2, '#ffffff'],
      [9, 11, 1, 2, '#ffffff'],
    ],
    extras: [
      [-3, 9, 3, 7, '#6d28d9'],
      [-4, 7, 2, 3, '#6d28d9'],
      [16, 9, 3, 7, '#6d28d9'],
      [18, 7, 2, 3, '#6d28d9'],
    ],
  },
]

/** Blend a #rrggbb colour toward white; the front face passes through this pale tint before the turn. */
export function tint(hex: string, amount: number): string {
  const n = Number.parseInt(hex.slice(1), 16)
  const mix = (c: number) => Math.round(c + (255 - c) * amount)
  const [r, g, b] = [mix(n >> 16), mix((n >> 8) & 0xff), mix(n & 0xff)]
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, '0')}`
}
