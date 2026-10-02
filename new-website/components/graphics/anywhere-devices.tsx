'use client'

import { motion } from 'motion/react'

import { easeInOut, easeOut } from '@/lib/motion'

/**
 * Line-drawn device illustrations for the "Runs anywhere" scene. Every device sits in a
 * 120×76 box, uses 1px strokes, `rx 6` bodies and `rx 2–3` parts, and has one activity LED.
 */

export type DeviceState = {
  /** The device is handling a call right now. */
  busy: boolean
  /** The worker process has crashed. */
  down: boolean
}

const BODY = { fill: 'var(--node)', stroke: 'var(--line-strong)' } as const
const PART = { fill: 'var(--faint)', stroke: 'var(--line-strong)' } as const
const round = { strokeLinecap: 'round', strokeLinejoin: 'round' } as const

function Led({ x, y, busy, down }: DeviceState & { x: number; y: number }) {
  return (
    <motion.circle
      cx={x}
      cy={y}
      r={2}
      initial={false}
      animate={{
        fill: down ? 'var(--fail)' : busy ? 'var(--ok)' : 'var(--line-strong)',
        opacity: busy && !down ? [1, 0.35, 1, 0.35, 1] : 1,
      }}
      transition={{ duration: busy ? 1.2 : 0.3 }}
    />
  )
}

/** Raspberry Pi board: GPIO header, SoC, RAM, USB and Ethernet ports, mounting holes. */
export function PiBoard(s: DeviceState) {
  return (
    <g {...round}>
      <rect x={6} y={8} width={104} height={60} rx={6} {...BODY} />
      <rect x={100} y={24} width={16} height={12} rx={2} {...BODY} />
      <rect x={100} y={42} width={16} height={16} rx={2} {...BODY} />
      <line x1={26} y1={15} x2={88} y2={15} stroke="var(--line-strong)" strokeWidth={2.2} strokeDasharray="0.1 4.4" />
      <line x1={26} y1={20} x2={88} y2={20} stroke="var(--line-strong)" strokeWidth={2.2} strokeDasharray="0.1 4.4" />
      {[
        [14, 16],
        [96, 16],
        [14, 60],
        [96, 60],
      ].map(([cx, cy]) => (
        <circle key={`${cx}-${cy}`} cx={cx} cy={cy} r={2.5} fill="none" stroke="var(--line)" />
      ))}
      <rect x={34} y={30} width={26} height={26} rx={3} {...PART} />
      <rect x={66} y={34} width={14} height={18} rx={2} fill="none" stroke="var(--line-strong)" />
      <Led x={24} y={60} {...s} />
    </g>
  )
}

/** Laptop whose screen shows three iOS simulators. */
export function MacSimulators(s: DeviceState) {
  return (
    <g {...round}>
      <rect x={16} y={4} width={88} height={58} rx={6} {...BODY} />
      <rect x={21} y={9} width={78} height={48} rx={2} fill="var(--faint)" stroke="var(--line)" />
      <path d="M4 62 H116 L113 69 Q112 72 109 72 H11 Q8 72 7 69 Z" {...BODY} />
      <line x1={52} y1={62} x2={68} y2={62} stroke="var(--line)" />
      {[35, 53, 71].map((x, i) => (
        <g key={x}>
          <motion.rect
            x={x}
            y={17}
            width={14}
            height={32}
            rx={3}
            fill="var(--node)"
            initial={false}
            animate={{ stroke: s.busy && i === 1 ? 'var(--foreground)' : 'var(--line-strong)' }}
            transition={{ duration: 0.3 }}
          />
          <line x1={x + 5} y1={45} x2={x + 9} y2={45} stroke="var(--line-strong)" />
        </g>
      ))}
      <Led x={60} y={6.5} {...s} />
    </g>
  )
}

const BLADES = 'M0 -3.5 C5 -9 10 -7 11 -3 M3.5 0 C9 5 7 10 3 11 M0 3.5 C-5 9 -10 7 -11 3 M-3.5 0 C-9 -5 -7 -10 -3 -11'

/** GPU box with two fans that spin while it serves a model. */
export function GpuBox(s: DeviceState) {
  return (
    <g {...round}>
      <rect x={2} y={18} width={5} height={40} rx={1.5} {...BODY} />
      <rect x={7} y={10} width={108} height={56} rx={6} {...BODY} />
      {[38, 80].map((cx) => (
        <g key={cx}>
          <circle cx={cx} cy={38} r={16} {...PART} />
          <g transform={`translate(${cx} 38)`}>
            <motion.path
              d={BLADES}
              fill="none"
              stroke="var(--line-strong)"
              initial={false}
              animate={s.busy && !s.down ? { rotate: [0, 360] } : { rotate: 360 }}
              transition={
                s.busy && !s.down
                  ? { duration: 1.1, ease: 'linear', repeat: Number.POSITIVE_INFINITY }
                  : { duration: 0.8, ease: easeOut }
              }
              style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
            />
          </g>
          <circle cx={cx} cy={38} r={3.5} fill="var(--node)" stroke="var(--line-strong)" />
        </g>
      ))}
      <line x1={104} y1={30} x2={104} y2={46} stroke="var(--line)" />
      <line x1={108} y1={30} x2={108} y2={46} stroke="var(--line)" />
      <Led x={106} y={19} {...s} />
    </g>
  )
}

/** Browser window: tab bar, address bar, page skeleton and a notification the backend can trigger. */
export function BrowserTab(s: DeviceState) {
  return (
    <g {...round}>
      <rect x={4} y={6} width={112} height={64} rx={6} {...BODY} />
      <line x1={4} y1={21} x2={116} y2={21} stroke="var(--line)" />
      {[13, 20, 27].map((cx) => (
        <circle key={cx} cx={cx} cy={13.5} r={2} fill="var(--line-strong)" />
      ))}
      <rect x={36} y={10} width={72} height={7} rx={3.5} fill="var(--faint)" stroke="var(--line)" />
      <rect x={14} y={30} width={56} height={4} rx={2} fill="var(--line)" />
      <rect x={14} y={39} width={40} height={4} rx={2} fill="var(--line)" />
      <rect x={14} y={48} width={48} height={4} rx={2} fill="var(--line)" />
      <motion.g
        initial={false}
        animate={{ opacity: s.busy ? 1 : 0, y: s.busy ? 0 : 4 }}
        transition={{ duration: 0.35, ease: easeOut }}
      >
        <rect x={70} y={48} width={38} height={14} rx={3} fill="var(--node)" stroke="var(--foreground)" />
        <line x1={76} y1={55} x2={100} y2={55} stroke="var(--foreground)" />
      </motion.g>
      <Led x={110} y={13.5} {...s} />
    </g>
  )
}

/** Ephemeral CI runner (dashed): a three-step job that runs while it is connected. */
export function CiRunner(s: DeviceState) {
  const steps = [30, 60, 90]
  return (
    <g {...round}>
      <rect x={6} y={8} width={108} height={60} rx={6} {...BODY} strokeDasharray="3 3" />
      <line x1={35} y1={32} x2={55} y2={32} stroke="var(--line-strong)" />
      <line x1={65} y1={32} x2={85} y2={32} stroke="var(--line-strong)" />
      {steps.map((cx, i) => (
        <motion.circle
          key={cx}
          cx={cx}
          cy={32}
          r={5}
          stroke="var(--line-strong)"
          initial={false}
          animate={{ fill: s.busy ? 'var(--foreground)' : 'var(--faint)' }}
          transition={{ duration: 0.25, delay: s.busy ? 0.9 + i * 0.3 : 0 }}
        />
      ))}
      <rect x={18} y={50} width={84} height={4} rx={2} fill="var(--faint)" stroke="var(--line)" />
      <motion.rect
        x={18}
        y={50}
        height={4}
        rx={2}
        fill="var(--foreground)"
        initial={false}
        animate={{ width: s.busy ? 84 : 0 }}
        transition={{ duration: s.busy ? 1.0 : 0.2, delay: s.busy ? 0.8 : 0, ease: easeInOut }}
      />
      <Led x={106} y={16} {...s} />
    </g>
  )
}

/** Host running a microVM (double-walled box) that an agent (sparkle) just started for untrusted code. */
export function MicroVm(s: DeviceState) {
  return (
    <g {...round}>
      <rect x={6} y={6} width={108} height={64} rx={6} {...BODY} />
      <path
        d="M17 13 Q17.6 17.4 22 18 Q17.6 18.6 17 23 Q16.4 18.6 12 18 Q16.4 17.4 17 13 Z"
        fill="var(--faint)"
        stroke="var(--line-strong)"
      />
      <path d="M23 22 Q30 30 36 30" fill="none" stroke="var(--line-strong)" strokeDasharray="2 3" />
      <motion.rect
        x={38}
        y={18}
        width={62}
        height={42}
        rx={4}
        fill="var(--faint)"
        initial={false}
        animate={{ stroke: s.busy ? 'var(--foreground)' : 'var(--line-strong)' }}
        transition={{ duration: 0.3 }}
      />
      <rect x={42} y={22} width={54} height={34} rx={2.5} fill="none" stroke="var(--line)" strokeDasharray="2 2.5" />
      <path d="M60 33 L55 39 L60 45 M78 33 L83 39 L78 45 M71 31 L67 47" fill="none" stroke="var(--line-strong)" />
      <Led x={106} y={63} {...s} />
    </g>
  )
}
