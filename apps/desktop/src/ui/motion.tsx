import { useEffect, useRef, useState, type ReactNode } from 'react'
import { motion, type MotionEase, type StyleDesc } from '@gpuix/react'

/**
 * Durations in seconds, the unit gpuix motion takes. Everything stays under
 * 300ms: in a chat client motion is feedback, never a show.
 */
export const DURATION = {
  /** Popovers, and anything on its way out. */
  fast: 0.12,
  /** Enter transitions and small reveals. */
  base: 0.18,
  /** The details panel sliding in. */
  panel: 0.28,
} as const

/** A strong ease-out: fast start, long settle. Fades and small reveals use it. */
export const EASE_OUT: MotionEase = [0.23, 1, 0.32, 1]
export const EASE_IN_OUT: MotionEase = [0.77, 0, 0.175, 1]
/**
 * The iOS sheet curve, for anything that travels a long way. A strong
 * ease-out covers most of a 280px slide in its first few frames, which reads
 * as a jump and a crawl however high the frame rate.
 */
export const EASE_DRAWER: MotionEase = [0.32, 0.72, 0, 1]

/**
 * gpuix has no exit animations: an element is gone the frame React drops it.
 * This keeps `show=false` mounted for `exitSeconds` so it can animate out first.
 */
export function usePresence(show: boolean, exitSeconds: number): boolean {
  const [mounted, setMounted] = useState(show)
  useEffect(() => {
    if (show) {
      setMounted(true)
      return
    }
    const timer = setTimeout(() => setMounted(false), exitSeconds * 1000)
    return () => clearTimeout(timer)
  }, [show, exitSeconds])
  return show || mounted
}

/** The last defined value, so content on its way out keeps painting while it fades. */
export function useHeld<T>(value: T | undefined): T | undefined {
  const held = useRef(value)
  if (value !== undefined) held.current = value
  return value ?? held.current
}

/**
 * For overlays driven by a nullable piece of state: `current` is the value to
 * render (the last one while it fades out), `open` says which way it is going.
 */
export function useLeaving<T>(value: T | null, exitSeconds: number): { current: T | null; open: boolean } {
  const mounted = usePresence(value !== null, exitSeconds)
  const held = useHeld(value ?? undefined)
  return { current: mounted ? (held ?? null) : null, open: value !== null }
}

/** Fades in on mount, fades out for `exit` seconds before unmounting. */
export function Fade({
  show = true,
  enter = DURATION.base,
  exit = DURATION.fast,
  style,
  children,
}: {
  show?: boolean
  enter?: number
  exit?: number
  style?: StyleDesc
  children: ReactNode
}) {
  const mounted = usePresence(show, exit)
  if (!mounted) return null
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: show ? 1 : 0 }}
      transition={{ duration: show ? enter : exit, ease: EASE_OUT }}
      style={{ ...style, ...(show ? {} : { pointerEvents: 'none' }) }}
    >
      {children}
    </motion.div>
  )
}

/**
 * Grows to `height` and fades in, collapses back before unmounting. The child
 * has to be exactly `height` tall: the box clips, it does not measure.
 */
export function Reveal({ show, height, duration = DURATION.base, children }: { show: boolean; height: number; duration?: number; children: ReactNode }) {
  const mounted = usePresence(show, duration)
  if (!mounted) return null
  return (
    <motion.div
      initial={{ height: 0, opacity: 0 }}
      animate={{ height: show ? height : 0, opacity: show ? 1 : 0 }}
      transition={{ duration, ease: EASE_OUT }}
      style={{ overflow: 'hidden', flexShrink: 0, display: 'flex', flexDirection: 'column' }}
    >
      {children}
    </motion.div>
  )
}
