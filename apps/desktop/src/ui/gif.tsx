import { mkdir, readdir, rename, rm } from 'node:fs/promises'
import { createContext, useContext, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { attachmentsDir } from '@messages/core'
import { ffmpegOnce, ffmpegSlot } from './ffmpeg'

/**
 * GIFs are played from here rather than handed to the renderer animated.
 * GPUI asks for a new frame on every paint while an animated image is on
 * screen, and gpuix rebuilds and re-lays out the whole tree for each one, so
 * a thread with a GIF in it ran at the display's refresh rate: sixty full
 * layouts a second to show ten frames of a sticker. Split into stills on
 * disk, the picture changes only when the GIF says so, and every bubble
 * showing the same file steps off one clock.
 */
export interface GifFrames {
  /** One PNG per frame, in order. */
  paths: string[]
  /** How long each frame stays up, in milliseconds. */
  delays: number[]
}

/** Browsers show a frame asking for less than this for a tenth of a second; so do we. */
const MIN_DELAY_MS = 20
const SLOW_FRAME_MS = 100

function framesDir(imagePath: string): string {
  return `${attachmentsDir}/${imagePath.split('/').pop()}.frames`
}

/** `ffprobe` prints one `pts,duration` line per frame; the delays are what the GIF asked for, clamped the way browsers clamp them. */
async function frameDelays(imagePath: string): Promise<number[] | null> {
  const proc = Bun.spawn(['ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_entries', 'frame=duration_time', '-of', 'csv=p=0', imagePath], { stdout: 'pipe', stderr: 'ignore' })
  const text = await new Response(proc.stdout).text()
  if ((await proc.exited) !== 0) return null
  const delays = text
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => Math.round(Number.parseFloat(line) * 1000))
  if (delays.some((delay) => !Number.isFinite(delay))) return null
  return delays.map((delay) => (delay < MIN_DELAY_MS ? SLOW_FRAME_MS : delay))
}

/**
 * The frames of a GIF as PNG files in `<cache>/<file>.frames/`, extracted
 * once with ffmpeg. Null for a still, a data URL, a file ffmpeg cannot read,
 * or a machine without it, in which case the caller shows the GIF itself.
 */
export async function gifFrames(imagePath: string): Promise<GifFrames | null> {
  if (imagePath.startsWith('data:') || !Bun.which('ffmpeg') || !Bun.which('ffprobe')) return null
  const dir = framesDir(imagePath)
  const index = `${dir}/index.json`
  if (await Bun.file(index).exists()) return Bun.file(index).json() as Promise<GifFrames>
  const done = await ffmpegOnce(index, async () => {
    try {
      const delays = await frameDelays(imagePath)
      if (!delays || delays.length < 2) return null
      const partial = `${dir}.part`
      await rm(partial, { recursive: true, force: true })
      await mkdir(partial, { recursive: true })
      const ok = await ffmpegSlot(async () => {
        const proc = Bun.spawn(['ffmpeg', '-y', '-v', 'error', '-i', imagePath, '-fps_mode', 'passthrough', `${partial}/%04d.png`], { stdout: 'ignore', stderr: 'ignore' })
        return (await proc.exited) === 0
      })
      const files = ok ? (await readdir(partial)).filter((name) => name.endsWith('.png')).sort() : []
      if (files.length !== delays.length) {
        await rm(partial, { recursive: true, force: true })
        return null
      }
      const frames: GifFrames = { paths: files.map((name) => `${dir}/${name}`), delays }
      await Bun.write(`${partial}/index.json`, JSON.stringify(frames))
      await rm(dir, { recursive: true, force: true })
      await rename(partial, dir)
      return index
    } catch {
      return null
    }
  })
  return done ? (Bun.file(index).json() as Promise<GifFrames>) : null
}

/** One clock per GIF file: whoever shows it subscribes, and the timer runs only while someone does. */
class GifClock {
  index = 0
  private timer: ReturnType<typeof setTimeout> | null = null
  private readonly listeners = new Set<() => void>()

  constructor(private readonly frames: GifFrames) {}

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    if (this.listeners.size === 1) this.schedule()
    return () => {
      this.listeners.delete(listener)
      if (this.listeners.size === 0 && this.timer) {
        clearTimeout(this.timer)
        this.timer = null
      }
    }
  }

  private schedule(): void {
    this.timer = setTimeout(() => {
      this.index = (this.index + 1) % this.frames.paths.length
      for (const listener of this.listeners) listener()
      this.schedule()
    }, this.frames.delays[this.index])
  }
}

const clocks = new Map<string, GifClock>()
const unsubscribed = () => () => undefined

function clockFor(key: string, frames: GifFrames): GifClock {
  let clock = clocks.get(key)
  if (!clock) {
    clock = new GifClock(frames)
    clocks.set(key, clock)
  }
  return clock
}

/**
 * Whether the row a GIF sits in is inside the list's painted window. The
 * thread mounts every row it holds, so without this a GIF forty screens up
 * would keep stepping and re-rendering along with the ones on screen.
 */
export const GifWindow = createContext(true)

/**
 * Every frame is its own file to the renderer, decoded the first time it is
 * painted, so stepping straight away showed a blank for each frame of the
 * first loop. The GIF itself stays up for this long while a hidden row of
 * every frame gets them all decoded; the atlas keeps them from then on.
 */
const WARM_MS = 1500
const warmed = new Set<string>()
const warmingOwned = new Set<string>()
const warmListeners = new Map<string, Set<() => void>>()

/** Whether the frames of `path` are decoded, and whether this caller is the one mounting the preloader. */
function useWarm(path: string | undefined, frames: GifFrames | null): { warm: boolean; owner: boolean } {
  const [, bump] = useState(0)
  const owner = useRef(false)
  useEffect(() => {
    if (!path || !frames || warmed.has(path)) return
    const listeners = warmListeners.get(path) ?? new Set<() => void>()
    warmListeners.set(path, listeners)
    const listener = () => bump((n) => n + 1)
    listeners.add(listener)
    let timer: ReturnType<typeof setTimeout> | null = null
    if (!warmingOwned.has(path)) {
      warmingOwned.add(path)
      owner.current = true
      timer = setTimeout(() => {
        timer = null
        warmed.add(path)
        warmingOwned.delete(path)
        warmListeners.delete(path)
        for (const waiting of listeners) waiting()
      }, WARM_MS)
      bump((n) => n + 1)
    }
    return () => {
      listeners.delete(listener)
      if (timer) {
        clearTimeout(timer)
        warmingOwned.delete(path)
      }
      owner.current = false
    }
  }, [path, frames])
  const warm = Boolean(path && warmed.has(path))
  return { warm, owner: owner.current && !warm }
}

export interface GifPlayback {
  /** What to paint: the current frame once warm, the GIF itself before that. */
  src: string | undefined
  /** Every frame, for this component to mount hidden while they decode; empty otherwise. */
  preload: string[]
}

/**
 * The frame to paint right now for a GIF at `imagePath`, or the path itself
 * while its frames are still being cut (or cannot be). Copies of one file
 * share one clock, so a spammed GIF steps in unison across the thread. Off
 * screen the frame freezes and the clock stops once nobody else is showing it.
 */
export function useGif(imagePath: string | undefined, animated: boolean): GifPlayback {
  const inView = useContext(GifWindow)
  const [frames, setFrames] = useState<{ path: string; frames: GifFrames | null } | null>(null)
  useEffect(() => {
    if (!imagePath || !animated) return
    let cancelled = false
    void gifFrames(imagePath).then((result) => {
      if (!cancelled) setFrames({ path: imagePath, frames: result })
    })
    return () => {
      cancelled = true
    }
  }, [imagePath, animated])
  const ready = imagePath && frames?.path === imagePath ? frames.frames : null
  const { warm, owner } = useWarm(ready ? imagePath : undefined, ready)
  const clock = ready && warm && imagePath ? clockFor(imagePath, ready) : null
  const index = useSyncExternalStore(clock && inView ? clock.subscribe : unsubscribed, () => (clock ? clock.index : 0), () => 0)
  if (!imagePath) return { src: undefined, preload: [] }
  return { src: ready && warm ? ready.paths[index] : imagePath, preload: owner && ready ? ready.paths : [] }
}

/** A one pixel, invisible row of every frame, so the renderer decodes them before they are stepped through. */
export function GifPreload({ paths }: { paths: string[] }) {
  if (paths.length === 0) return null
  return (
    <div style={{ position: 'absolute', top: 0, left: 0, width: 1, height: 1, opacity: 0, overflow: 'hidden', display: 'flex', flexDirection: 'row', pointerEvents: 'none' }}>
      {paths.map((path) => (
        <img key={path} src={path} style={{ width: 1, height: 1, flexShrink: 0 }} />
      ))}
    </div>
  )
}
