import type { Face } from '../shared/cube-mark'

/**
 * Pixel Diwali faces for the cube-turn mark: a diya, an akash kandil (sky lantern), a laddoo and an anar (flowerpot
 * firework). Every shape is drawn on a 16×16 head grid; accessories may reach 4 cells past it on any side. A rect is
 * [x, y, width, height, fill?].
 */

const SAFFRON = '#ff9933'
const GOLD = '#f2c14e'
const CORE = '#fff4c2'
const FLAME_RED = '#e8452c'

const DIYA: Face = {
  name: 'Diya',
  color: '#b85a2a',
  // The clay bowl fills the lower part of the grid; the flame burns in the space above it.
  head: [
    [0, 6, 16, 6],
    [1, 12, 14, 2],
    [3, 14, 10, 2],
  ],
  marks: [
    [0, 6, 16, 1, '#7c3a18'],
    [2, 13, 1, 1, GOLD],
    [5, 14, 1, 1, GOLD],
    [10, 14, 1, 1, GOLD],
    [13, 13, 1, 1, GOLD],
  ],
  eyes: [
    [4, 8, 2, 3, '#2a1206'],
    [10, 8, 2, 3, '#2a1206'],
  ],
  details: [
    [7, 11, 2, 1, '#2a1206'],
    [7, 0, 2, 1, '#ffe08a'],
    [6, 1, 4, 2, SAFFRON],
    [5, 3, 6, 2, SAFFRON],
    [6, 5, 4, 1, FLAME_RED],
    [7, 2, 2, 3, CORE],
  ],
  extras: [
    [-3, -2, 1, 1, GOLD],
    [18, 0, 1, 1, GOLD],
    [-2, 3, 1, 1, CORE],
    [17, -3, 1, 1, CORE],
    [16, 7, 3, 2, '#b85a2a'],
  ],
}

const KANDIL: Face = {
  name: 'Akash kandil',
  color: '#d63384',
  head: [
    [6, 0, 4, 1],
    [4, 1, 8, 1],
    [2, 2, 12, 2],
    [0, 4, 16, 7],
    [2, 11, 12, 2],
    [4, 13, 8, 1],
    [6, 14, 4, 1],
  ],
  marks: [
    [0, 5, 16, 1, GOLD],
    [2, 11, 12, 1, GOLD],
    [7, 0, 2, 15, '#b8246d'],
  ],
  eyes: [
    [4, 7, 2, 2, '#ffffff'],
    [10, 7, 2, 2, '#ffffff'],
  ],
  details: [[7, 9, 2, 1, '#5a0f2e']],
  extras: [
    [7, -4, 2, 4, '#e9d3a8'],
    [5, 15, 1, 4, GOLD],
    [7, 15, 2, 5, GOLD],
    [10, 15, 1, 4, GOLD],
  ],
}

const LADDOO: Face = {
  name: 'Laddoo',
  color: '#f2a33a',
  head: [
    [3, 0, 10, 1],
    [1, 1, 14, 2],
    [0, 3, 16, 10],
    [1, 13, 14, 2],
    [3, 15, 10, 1],
  ],
  marks: [
    [3, 2, 1, 1, '#d9861b'],
    [11, 2, 1, 1, '#d9861b'],
    [2, 10, 1, 1, '#d9861b'],
    [13, 11, 1, 1, '#d9861b'],
    [6, 13, 1, 1, '#d9861b'],
    [9, 14, 1, 1, '#d9861b'],
  ],
  eyes: [
    [5, 6, 2, 3, '#3a1d0a'],
    [9, 6, 2, 3, '#3a1d0a'],
  ],
  details: [
    [6, 11, 4, 1, '#3a1d0a'],
    [5, 10, 1, 1, '#3a1d0a'],
    [10, 10, 1, 1, '#3a1d0a'],
  ],
  extras: [
    [7, -2, 2, 2, '#7bb662'],
    [-2, 16, 20, 1, '#c9ced6'],
    [0, 17, 16, 1, '#a3a9b3'],
  ],
}

const ANAR: Face = {
  name: 'Anar',
  color: '#2f6fd6',
  head: [
    [5, 2, 6, 2],
    [4, 4, 8, 3],
    [3, 7, 10, 4],
    [2, 11, 12, 5],
  ],
  marks: [
    [4, 6, 8, 1, GOLD],
    [2, 13, 12, 1, GOLD],
  ],
  eyes: [
    [5, 8, 2, 2, '#ffffff'],
    [9, 8, 2, 2, '#ffffff'],
  ],
  details: [
    [7, 11, 2, 1, '#0f2a5c'],
    [7, 0, 2, 2, '#ffd166'],
  ],
  // The fountain of sparks.
  extras: [
    [7, -4, 2, 2, CORE],
    [4, -3, 1, 2, GOLD],
    [11, -3, 1, 2, GOLD],
    [2, -1, 1, 1, SAFFRON],
    [13, -1, 1, 1, SAFFRON],
    [5, -1, 1, 1, CORE],
    [10, -1, 1, 1, CORE],
    [0, 1, 1, 1, GOLD],
    [15, 1, 1, 1, GOLD],
  ],
}

export const FACES: readonly Face[] = [DIYA, KANDIL, LADDOO, ANAR]
