import type { Face, Px } from '../shared/cube-mark'

/**
 * Pixel harvest faces for the cube-turn mark, in the Halloween monsters' format. Every shape is drawn on a 16×16 head
 * grid; accessories may reach 4 cells past it on any side. A rect is [x, y, width, height, fill?].
 */
const RED = '#c9472f'
const ORANGE = '#e5852e'
const AMBER = '#ebb24b'
const CREAM = '#f7e3b5'

const SQUARE: readonly Px[] = [[0, 0, 16, 16]]
const EYES = (fill: string): readonly Px[] => [
  [5, 7, 2, 3, fill],
  [9, 7, 2, 3, fill],
]

/**
 * One ring of the tail fan behind the turkey's head: a stepped arch from `inset` cells inside the 24-cell canvas
 * down to row 9. Rings are stacked outer-first, so each inner ring leaves a one-cell band of the one behind it.
 */
function arch(inset: number, fill: string): Px[] {
  const left = -4 + inset
  const width = 24 - inset * 2
  const top = -4 + inset
  return [
    [left + 2, top, width - 4, 1, fill],
    [left + 1, top + 1, width - 2, 1, fill],
    [left, top + 2, width, 9 - (top + 2), fill],
  ]
}

const TURKEY: Face = {
  name: 'Turkey',
  color: '#9a643b',
  head: [
    [2, 0, 12, 1],
    [1, 1, 14, 1],
    [0, 2, 16, 14],
  ],
  marks: [
    [3, 4, 4, 4, '#f5f5f5'],
    [9, 4, 4, 4, '#f5f5f5'],
  ],
  eyes: [
    [5, 6, 2, 2, '#1d130c'],
    [9, 6, 2, 2, '#1d130c'],
  ],
  details: [
    [8, 10, 2, 5, '#e04a3c'],
    [8, 15, 1, 1, '#e04a3c'],
    [6, 9, 4, 2, '#f4b53c'],
    [7, 11, 1, 1, '#f4b53c'],
  ],
  // The fan: red outside, then orange, then amber, with cream feather tips along the outer edge.
  extras: [
    ...arch(0, RED),
    ...arch(1, ORANGE),
    ...arch(2, AMBER),
    [0, -4, 2, 1, CREAM],
    [5, -4, 2, 1, CREAM],
    [9, -4, 2, 1, CREAM],
    [14, -4, 2, 1, CREAM],
    [-4, 0, 1, 2, CREAM],
    [19, 0, 1, 2, CREAM],
  ],
}

const PIE: Face = {
  name: 'Pumpkin pie',
  color: '#d9822b',
  head: SQUARE,
  marks: [
    [0, 0, 16, 3, '#c99a5b'],
    [0, 0, 16, 1, '#b07e45'],
    [1, 3, 2, 1, '#c99a5b'],
    [5, 3, 2, 1, '#c99a5b'],
    [9, 3, 2, 1, '#c99a5b'],
    [13, 3, 2, 1, '#c99a5b'],
    [3, 10, 2, 1, '#eaa25f'],
    [11, 10, 2, 1, '#eaa25f'],
  ],
  eyes: EYES('#3a1d0a'),
  details: [
    [6, 12, 4, 1, '#3a1d0a'],
    [5, 11, 1, 1, '#3a1d0a'],
    [10, 11, 1, 1, '#3a1d0a'],
  ],
  extras: [
    [4, -2, 8, 2, '#fbf4e6'],
    [6, -4, 4, 2, '#fbf4e6'],
  ],
}

const LEAF: Face = {
  name: 'Maple leaf',
  color: RED,
  head: [
    [6, 0, 4, 2],
    [5, 2, 6, 2],
    [1, 2, 3, 2],
    [12, 2, 3, 2],
    [0, 4, 16, 7],
    [2, 11, 12, 2],
    [4, 13, 8, 1],
    [6, 14, 4, 1],
  ],
  eyes: [
    [5, 6, 2, 3, '#fdf2e0'],
    [9, 6, 2, 3, '#fdf2e0'],
  ],
  details: [
    [6, 10, 1, 1, '#5c1810'],
    [9, 10, 1, 1, '#5c1810'],
    [7, 11, 2, 1, '#5c1810'],
  ],
  extras: [
    [7, 15, 2, 4, '#6b3f22'],
    [-4, -2, 2, 2, AMBER],
    [18, 1, 2, 2, ORANGE],
    [-3, 13, 2, 2, ORANGE],
  ],
}

const CORN: Face = {
  name: 'Corn',
  color: '#ecc54c',
  head: [
    [3, 0, 10, 1],
    [2, 1, 12, 14],
    [3, 15, 10, 1],
  ],
  marks: [1, 3, 5, 13].flatMap((y) => [3, 5, 7, 9, 11].map((x): Px => [x, y, 1, 1, '#d4a332'])),
  eyes: EYES('#2a1d08'),
  details: [
    [6, 10, 1, 1, '#2a1d08'],
    [9, 10, 1, 1, '#2a1d08'],
    [7, 11, 2, 1, '#2a1d08'],
  ],
  extras: [
    [-3, 9, 2, 7, '#4a7530'],
    [17, 9, 2, 7, '#4a7530'],
    [-2, 5, 3, 11, '#5f8f3a'],
    [15, 5, 3, 11, '#5f8f3a'],
    [6, -3, 1, 3, '#c9a35a'],
    [8, -4, 1, 4, '#d9b56a'],
    [10, -3, 1, 3, '#c9a35a'],
  ],
}

export const FACES: readonly Face[] = [TURKEY, PIE, LEAF, CORN]
