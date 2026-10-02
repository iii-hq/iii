'use client'

import { useEffect, useRef } from 'react'

import { cn } from '@/lib/utils'

/*
 * Hero ASCII scene: the iii mark as a solid 3D sculpture (the engine) with worker cubes orbiting
 * it. Every worker holds a line (its WebSocket) to the engine; packets travel worker → engine →
 * worker, and every few seconds a worker flies in and joins, or leaves. Rendered per character
 * cell with exact ray/box intersection under an orthographic camera: faces toward the viewer get
 * a density ramp, tilted faces get slope glyphs, silhouettes and creases get edge glyphs. The
 * scene tilts toward the pointer, and cells near the pointer scramble into code glyphs.
 */

type V3 = [number, number, number]
type Box = { c: V3; h: V3 }

// The iii mark in world units (logo proportions: dot 0.5, gap 0.25, stem 1.5, bar gap 0.25).
const LOGO: Box[] = []
for (const x of [-0.75, 0, 0.75]) {
  LOGO.push({ c: [x, 0.875, 0], h: [0.25, 0.25, 0.25] }, { c: [x, -0.375, 0], h: [0.25, 0.75, 0.25] })
}

const WORKERS = 7
const ORBIT_R = 2.35
const CUBE_H = 0.16
const LIGHT = (() => {
  const v: V3 = [-0.45, 0.7, 0.6]
  const m = Math.hypot(...v)
  return v.map((x) => x / m) as V3
})()

const RAMP = ['.', '.', ':', ':', '-', '=', '+']
const HOT = '01{}[]<>/\\=;:*+#'

const JOIN_EVERY = 4200
const JOIN_MS = 1100
const PACKET_EVERY = 1500
const PACKET_MS = 1300

function edgeGlyph(dx: number, dy: number) {
  // Glyph drawn along direction (dx, dy) in screen space (y down).
  const oct = Math.round((Math.atan2(dy, dx) / Math.PI) * 4 + 8) % 8
  return oct === 0 || oct === 4 ? '-' : oct === 2 || oct === 6 ? '|' : oct === 1 || oct === 5 ? '\\' : '/'
}

function hash(a: number, b: number) {
  let h = (a * 374761393 + b * 668265263) | 0
  h = (h ^ (h >>> 13)) * 1274126177
  return ((h ^ (h >>> 16)) >>> 0) / 4294967295
}

const ease = (k: number) => 1 - (1 - k) ** 3

type Worker = { angle: number; y: number; spin: number; joined: number }

export function AsciiEngine({ className, focus = 0.56 }: { className?: string; focus?: number }) {
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    let width = 0
    let height = 0
    let dpr = 1
    let fontSize = 11
    let font = ''
    let color = '#fff'
    let cw = 7
    let ch = 13
    let cols = 0
    let rows = 0
    let ppu = 100
    let ox = 0
    let oy = 0
    let depth = new Float32Array(0)
    let glyph: string[] = []
    let alpha = new Float32Array(0)
    let obj = new Int16Array(0)
    let heat = new Float32Array(0)
    let visible = true
    let raf = 0
    let started = Number.NaN
    let fontsReady = false
    const pointer = { x: -1e4, y: -1e4, active: false, nx: 0, ny: 0 }
    const tilt = { yaw: 0, pitch: 0 }

    const workers: Worker[] = Array.from({ length: WORKERS }, (_, k) => ({
      angle: (k / WORKERS) * Math.PI * 2 + 0.3,
      y: [0.9, -0.6, 0.2, -1.1, 1.2, -0.2, 0.55][k],
      spin: k * 0.9,
      joined: reduce ? 1 : 0,
    }))

    const readStyle = () => {
      const cs = getComputedStyle(canvas)
      color = cs.color
      font = `${fontSize}px ${cs.fontFamily}`
    }

    const layout = () => {
      ctx.font = font
      cw = ctx.measureText('M').width
      ch = fontSize * 1.25
      cols = Math.ceil(width / cw)
      rows = Math.ceil(height / ch)
      const n = cols * rows
      depth = new Float32Array(n)
      glyph = new Array(n).fill(' ')
      alpha = new Float32Array(n)
      obj = new Int16Array(n)
      heat = new Float32Array(n)
      ppu = Math.min((height * 0.42) / 2.25, (width * Math.min(focus, 1 - focus) * 1.9) / (2 * (ORBIT_R + 0.4)))
      ox = width * focus
      oy = height * 0.5
    }

    const resize = () => {
      const rect = canvas.getBoundingClientRect()
      if (!rect.width || !rect.height) return
      width = rect.width
      height = rect.height
      dpr = Math.min(window.devicePixelRatio || 1, 2)
      canvas.width = Math.round(width * dpr)
      canvas.height = Math.round(height * dpr)
      fontSize = width < 520 ? 8.5 : 10.5
      readStyle()
      layout()
      requestFrame()
    }

    const frame = (now: number) => {
      raf = 0
      if (!fontsReady || !cols) return
      if (Number.isNaN(started)) started = now
      const time = reduce ? 0 : now - started

      // Scene rotation: gentle sway plus a tilt toward the pointer.
      const targetYaw = (reduce ? 0 : 0.32 * Math.sin(time / 4200)) + pointer.nx * 0.28
      const targetPitch = 0.26 + pointer.ny * 0.14
      tilt.yaw += (targetYaw - tilt.yaw) * (reduce ? 1 : 0.06)
      tilt.pitch += (targetPitch - tilt.pitch) * (reduce ? 1 : 0.06)
      const cy = Math.cos(tilt.yaw)
      const sy = Math.sin(tilt.yaw)
      const cp = Math.cos(tilt.pitch)
      const sp = Math.sin(tilt.pitch)
      // world → camera: yaw about y, then pitch about x.
      const toCam = (p: V3): V3 => {
        const x = p[0] * cy + p[2] * sy
        const z = -p[0] * sy + p[2] * cy
        return [x, p[1] * cp - z * sp, p[1] * sp + z * cp]
      }
      // camera → world (inverse rotation).
      const toWorld = (q: V3): V3 => {
        const y = q[1] * cp + q[2] * sp
        const z = -q[1] * sp + q[2] * cp
        return [q[0] * cy - z * sy, y, q[0] * sy + z * cy]
      }
      const dirW = toWorld([0, 0, -1])

      // Worker positions (orbit + join/leave choreography).
      const orbit = reduce ? 0 : time / 26000
      const joinCycle = Math.floor(time / JOIN_EVERY)
      const joinPhase = (time % JOIN_EVERY) / JOIN_MS
      const intro = Math.min(1, time / 2600)
      const positions: V3[] = []
      for (let k = 0; k < WORKERS; k++) {
        const w = workers[k]
        if (!reduce) {
          // Intro: workers join one after another. Afterwards worker 0 leaves and rejoins.
          const introJoin = Math.min(1, Math.max(0, intro * WORKERS - k))
          w.joined = introJoin
          if (k === 0 && time > 3200) {
            const leaving = joinCycle % 2 === 1
            const p = Math.min(1, joinPhase)
            w.joined = leaving ? 1 - ease(p) : ease(p)
          }
        }
        const a = w.angle + orbit * Math.PI * 2
        const r = ORBIT_R + (1 - w.joined) * 2.6
        positions.push([Math.cos(a) * r, w.y * (0.6 + 0.4 * w.joined), Math.sin(a) * r])
      }

      const n = cols * rows
      depth.fill(Number.POSITIVE_INFINITY)
      obj.fill(0)
      const spinT = reduce ? 0 : time / 2400

      // Ray-cast every cell.
      for (let r = 0; r < rows; r++) {
        for (let c = 0; c < cols; c++) {
          const u = (c * cw + cw / 2 - ox) / ppu
          const v = -(r * ch + ch / 2 - oy) / ppu
          const o = toWorld([u, v, 10])
          let bestT = Number.POSITIVE_INFINITY
          let bestN: V3 = [0, 0, 1]
          let bestObj = 0
          for (let b = 0; b < LOGO.length; b++) {
            const hit = rayBox(o, dirW, LOGO[b].c, LOGO[b].h)
            if (hit && hit.t < bestT) {
              bestT = hit.t
              bestN = hit.n
              bestObj = 1 + b
            }
          }
          for (let k = 0; k < WORKERS; k++) {
            if (workers[k].joined <= 0.001) continue
            const p = positions[k]
            const ang = workers[k].spin + spinT
            const ca = Math.cos(ang)
            const sa = Math.sin(ang)
            // Ray into the cube's local frame (spin about y).
            const lx = o[0] - p[0]
            const lz = o[2] - p[2]
            const lo: V3 = [lx * ca - lz * sa, o[1] - p[1], lx * sa + lz * ca]
            const ld: V3 = [dirW[0] * ca - dirW[2] * sa, dirW[1], dirW[0] * sa + dirW[2] * ca]
            const hit = rayBox(lo, ld, [0, 0, 0], [CUBE_H, CUBE_H, CUBE_H])
            if (hit && hit.t < bestT) {
              bestT = hit.t
              const nl = hit.n
              bestN = [nl[0] * ca + nl[2] * sa, nl[1], -nl[0] * sa + nl[2] * ca]
              bestObj = 100 + k
            }
          }
          const i = r * cols + c
          if (!bestObj) continue
          depth[i] = bestT
          obj[i] = bestObj
          const nc = toCam(bestN)
          const diffuse = Math.max(0, nc[0] * LIGHT[0] + nc[1] * LIGHT[1] + nc[2] * LIGHT[2])
          const lum = Math.min(1, (0.14 + 0.86 * diffuse) * (bestObj >= 100 ? 0.95 : 1))
          if (nc[2] > 0.8) {
            glyph[i] = RAMP[Math.min(RAMP.length - 1, Math.floor(lum * RAMP.length))]
            alpha[i] = 0.1 + 0.55 * lum
          } else {
            // Contour of a tilted face runs perpendicular to its screen-space normal.
            glyph[i] = edgeGlyph(-nc[1], -nc[0])
            alpha[i] = 0.16 + 0.6 * lum
          }
        }
      }

      // Silhouettes and creases.
      const outline = new Uint8Array(n)
      for (let r = 1; r < rows - 1; r++) {
        for (let c = 1; c < cols - 1; c++) {
          const i = r * cols + c
          if (!obj[i]) continue
          const same = (j: number) => obj[j] !== 0 && Math.abs(depth[j] - depth[i]) < 0.12 && sameGroup(obj[i], obj[j])
          const l = same(i - 1)
          const rt = same(i + 1)
          const u = same(i - cols)
          const dn = same(i + cols)
          if (l && rt && u && dn) continue
          const gx = (rt ? 1 : 0) - (l ? 1 : 0)
          const gy = (dn ? 1 : 0) - (u ? 1 : 0)
          if (!gx && !gy) continue
          outline[i] = 1
          glyph[i] = edgeGlyph(-gy, gx)
          alpha[i] = obj[i] >= 100 ? 0.9 : 0.8
        }
      }

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
      ctx.clearRect(0, 0, width, height)
      ctx.font = font
      ctx.textBaseline = 'top'
      ctx.fillStyle = color
      const buckets: [string, number, number][][] = Array.from({ length: 10 }, () => [])
      const put = (g: string, a: number, c: number, r: number) => {
        if (a >= 0.04) buckets[Math.min(9, Math.floor(a * 10))].push([g, c * cw, r * ch])
      }

      // WebSocket lines: worker → engine centre, depth-tested against the solids.
      const overlay = new Map<number, [string, number]>()
      const project = (p: V3) => {
        const q = toCam(p)
        return { x: q[0] * ppu + ox, y: -q[1] * ppu + oy, z: 10 - q[2] }
      }
      const engine = project([0, 0.1, 0])
      const packetPos = (k: number, t: number) => {
        const a = project(positions[k])
        return { x: a.x + (engine.x - a.x) * t, y: a.y + (engine.y - a.y) * t, z: a.z + (engine.z - a.z) * t }
      }
      for (let k = 0; k < WORKERS; k++) {
        const w = workers[k]
        if (w.joined < 0.05) continue
        const a = project(positions[k])
        const drawn = Math.min(1, w.joined * 1.2)
        const steps = Math.ceil(Math.max(Math.abs(engine.x - a.x) / cw, Math.abs(engine.y - a.y) / ch))
        const g = edgeGlyph(engine.x - a.x, engine.y - a.y)
        for (let s = 0; s <= steps * drawn; s++) {
          const t = s / steps
          const x = a.x + (engine.x - a.x) * t
          const y = a.y + (engine.y - a.y) * t
          const z = a.z + (engine.z - a.z) * t
          const c = Math.floor(x / cw)
          const r = Math.floor(y / ch)
          if (c < 0 || r < 0 || c >= cols || r >= rows) continue
          const i = r * cols + c
          if (z < depth[i] - 0.02 && !overlay.has(i)) overlay.set(i, [g, 0.32])
        }
      }

      // Packets: worker A → engine → worker B.
      if (!reduce && time > 2800) {
        const cycle = Math.floor(time / PACKET_EVERY)
        const p = ((time % PACKET_EVERY) / PACKET_MS) * 2
        const from = Math.floor(hash(cycle, 1) * WORKERS)
        const to = (from + 1 + Math.floor(hash(cycle, 2) * (WORKERS - 1))) % WORKERS
        if (p <= 2 && workers[from].joined > 0.9 && workers[to].joined > 0.9) {
          for (let trail = 0; trail < 4; trail++) {
            const pp = p - trail * 0.045
            if (pp < 0) continue
            const pos = pp <= 1 ? packetPos(from, ease(pp)) : packetPos(to, 1 - ease(Math.min(1, pp - 1)))
            const c = Math.floor(pos.x / cw)
            const r = Math.floor(pos.y / ch)
            if (c < 0 || r < 0 || c >= cols || r >= rows) continue
            const i = r * cols + c
            if (pos.z < depth[i] - 0.02) overlay.set(i, [trail === 0 ? '@' : trail === 1 ? 'o' : '.', 1 - trail * 0.2])
          }
        }
      }

      if (pointer.active) heatAround()

      const tick = Math.floor(now / 90)
      let hot = false
      for (let r = 0; r < rows; r++) {
        for (let c = 0; c < cols; c++) {
          const i = r * cols + c
          let g = obj[i] ? glyph[i] : ' '
          let a = obj[i] ? alpha[i] : 0
          const over = overlay.get(i)
          if (over) {
            g = over[0]
            a = over[1]
          }
          const hv = heat[i]
          if (hv > 0.06) {
            hot = true
            if (obj[i] || over || hv > 0.3) {
              g = HOT[Math.floor(hash(i, tick) * HOT.length)]
              a = obj[i] || over ? Math.min(1, a + hv * 0.85) : hv * 0.35
            }
            heat[i] = hv * 0.9
          } else if (hv > 0) heat[i] = 0
          if (g !== ' ') put(g, a, c, r)
        }
      }
      for (let b = 0; b < 10; b++) {
        const list = buckets[b]
        if (!list.length) continue
        ctx.globalAlpha = (b + 0.5) / 10
        for (const [g, x, y] of list) ctx.fillText(g, x, y)
      }
      ctx.globalAlpha = 1

      if (visible && (!reduce || hot || pointer.active)) requestFrame()
    }

    const heatAround = () => {
      const radius = 70
      const c0 = Math.max(0, Math.floor((pointer.x - radius) / cw))
      const c1 = Math.min(cols - 1, Math.ceil((pointer.x + radius) / cw))
      const r0 = Math.max(0, Math.floor((pointer.y - radius) / ch))
      const r1 = Math.min(rows - 1, Math.ceil((pointer.y + radius) / ch))
      for (let r = r0; r <= r1; r++) {
        for (let c = c0; c <= c1; c++) {
          const d = Math.hypot(c * cw + cw / 2 - pointer.x, r * ch + ch / 2 - pointer.y)
          if (d < radius) {
            const i = r * cols + c
            heat[i] = Math.max(heat[i], (1 - d / radius) ** 1.5)
          }
        }
      }
    }

    function requestFrame() {
      if (!raf) raf = requestAnimationFrame(frame)
    }

    const onMove = (e: PointerEvent) => {
      const rect = canvas.getBoundingClientRect()
      pointer.x = e.clientX - rect.left
      pointer.y = e.clientY - rect.top
      pointer.active = pointer.x >= 0 && pointer.y >= 0 && pointer.x <= width && pointer.y <= height
      // Tilt follows the pointer anywhere in the viewport, gently.
      pointer.nx = (e.clientX / window.innerWidth - 0.5) * 2
      pointer.ny = (e.clientY / window.innerHeight - 0.5) * 2
      requestFrame()
    }
    const onLeave = () => {
      pointer.active = false
      pointer.nx = 0
      pointer.ny = 0
    }

    let resizeTimer = 0
    const ro = new ResizeObserver(() => {
      window.clearTimeout(resizeTimer)
      resizeTimer = window.setTimeout(resize, cols ? 120 : 0)
    })
    ro.observe(canvas)
    const io = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting
      if (visible) requestFrame()
    })
    io.observe(canvas)
    const mo = new MutationObserver(() => {
      readStyle()
      requestFrame()
    })
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] })
    window.addEventListener('pointermove', onMove, { passive: true })
    document.documentElement.addEventListener('pointerleave', onLeave)
    document.fonts.ready.then(() => {
      fontsReady = true
      readStyle()
      if (width) layout()
      requestFrame()
    })

    return () => {
      cancelAnimationFrame(raf)
      window.clearTimeout(resizeTimer)
      ro.disconnect()
      io.disconnect()
      mo.disconnect()
      window.removeEventListener('pointermove', onMove)
      document.documentElement.removeEventListener('pointerleave', onLeave)
    }
  }, [focus])

  return (
    <canvas ref={canvasRef} aria-hidden className={cn('block h-full w-full font-mono text-foreground', className)} />
  )
}

/** Logo boxes of the same bar share faces, so they outline as one bar; cubes outline on their own. */
function sameGroup(a: number, b: number) {
  if (a >= 100 || b >= 100) return a === b
  return Math.floor((a - 1) / 2) === Math.floor((b - 1) / 2)
}

/** Slab test; returns entry distance and the entry face normal. */
function rayBox(o: V3, d: V3, c: V3, h: V3): { t: number; n: V3 } | null {
  let tmin = Number.NEGATIVE_INFINITY
  let tmax = Number.POSITIVE_INFINITY
  let axis = 0
  let sign = 1
  for (let k = 0; k < 3; k++) {
    const lo = c[k] - h[k] - o[k]
    const hi = c[k] + h[k] - o[k]
    if (Math.abs(d[k]) < 1e-9) {
      if (lo > 0 || hi < 0) return null
      continue
    }
    let t1 = lo / d[k]
    let t2 = hi / d[k]
    let s = -1
    if (t1 > t2) {
      const tmp = t1
      t1 = t2
      t2 = tmp
      s = 1
    }
    if (t1 > tmin) {
      tmin = t1
      axis = k
      sign = s
    }
    tmax = Math.min(tmax, t2)
    if (tmin > tmax) return null
  }
  if (tmax < 0) return null
  const n: V3 = [0, 0, 0]
  n[axis] = sign
  return { t: tmin, n }
}
