/**
 * Map snapshots for the details panel, rendered by Apple's own MapKit
 * (`MKMapSnapshotter`) so the client shows Apple Maps' style without touching
 * a tile server. `snapshot.swift` is compiled once to a binary keyed by its
 * source hash; a launchd agent can run it headless, with no window and no
 * Automation prompt.
 */

import { createHash } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { mkdir, rename } from 'node:fs/promises'
import path from 'node:path'

export interface SnapshotRequest {
  lat: number
  lon: number
  /** Points. The PNG is `w * scale` by `h * scale` pixels. */
  w: number
  h: number
  scale: number
  dark: boolean
  /** Metres shown across the wider side. */
  span: number
}

const LIMITS = { size: [32, 1024], scale: [1, 3], span: [100, 50_000] } as const
const DEFAULT_SPAN = 800

function numberParam(params: URLSearchParams, name: string): number | null {
  const raw = params.get(name)
  if (raw === null || raw.trim() === '') return null
  const value = Number(raw)
  return Number.isFinite(value) ? value : null
}

/** Answers the request, or the reason it is refused. Coordinates are rounded to five decimals (about a metre), which is also the cache key's precision. */
export function parseSnapshotRequest(params: URLSearchParams): SnapshotRequest | { error: string } {
  const lat = numberParam(params, 'lat')
  const lon = numberParam(params, 'lon')
  if (lat === null || lat < -90 || lat > 90) return { error: 'lat must be a number between -90 and 90' }
  if (lon === null || lon < -180 || lon > 180) return { error: 'lon must be a number between -180 and 180' }
  const w = numberParam(params, 'w')
  const h = numberParam(params, 'h')
  const [minSize, maxSize] = LIMITS.size
  if (w === null || !Number.isInteger(w) || w < minSize || w > maxSize) return { error: `w must be an integer from ${minSize} to ${maxSize}` }
  if (h === null || !Number.isInteger(h) || h < minSize || h > maxSize) return { error: `h must be an integer from ${minSize} to ${maxSize}` }
  const scale = numberParam(params, 'scale') ?? 2
  if (!Number.isInteger(scale) || scale < LIMITS.scale[0] || scale > LIMITS.scale[1]) return { error: 'scale must be 1, 2 or 3' }
  const darkParam = params.get('dark') ?? '0'
  if (darkParam !== '0' && darkParam !== '1') return { error: 'dark must be 0 or 1' }
  const span = numberParam(params, 'span') ?? DEFAULT_SPAN
  if (span < LIMITS.span[0] || span > LIMITS.span[1]) return { error: `span must be from ${LIMITS.span[0]} to ${LIMITS.span[1]} metres` }
  const round = (value: number) => Math.round(value * 1e5) / 1e5
  return { lat: round(lat), lon: round(lon), w, h, scale, dark: darkParam === '1', span: Math.round(span) }
}

export function snapshotKey(request: SnapshotRequest): string {
  return [request.lat.toFixed(5), request.lon.toFixed(5), request.w, request.h, request.scale, request.dark ? 'dark' : 'light', request.span].join(':')
}

export type Renderer = (request: SnapshotRequest) => Promise<Uint8Array>

/** A short TTL: the map under a coordinate barely changes, but Apple's labels and closures do. */
export const SNAPSHOT_TTL_MS = 10 * 60_000
const MAX_ENTRIES = 64

/** Renders each key once: a request that arrives while the same one is rendering waits for it. A failure is not cached. */
export class SnapshotCache {
  private entries = new Map<string, { png: Uint8Array; at: number }>()
  private pending = new Map<string, Promise<Uint8Array>>()

  constructor(
    private readonly render: Renderer,
    private readonly now: () => number = Date.now,
    private readonly ttlMs = SNAPSHOT_TTL_MS,
  ) {}

  get size(): number {
    return this.entries.size
  }

  async get(request: SnapshotRequest): Promise<Uint8Array> {
    const key = snapshotKey(request)
    const entry = this.entries.get(key)
    if (entry && this.now() - entry.at < this.ttlMs) return entry.png
    this.entries.delete(key)
    const inFlight = this.pending.get(key)
    if (inFlight) return inFlight
    const rendering = this.render(request)
      .then((png) => {
        this.entries.set(key, { png, at: this.now() })
        while (this.entries.size > MAX_ENTRIES) this.entries.delete(this.entries.keys().next().value!)
        return png
      })
      .finally(() => this.pending.delete(key))
    this.pending.set(key, rendering)
    return rendering
  }
}

const SOURCE = path.join(import.meta.dir, 'snapshot.swift')
const RENDER_TIMEOUT_MS = 25_000

let helper: Promise<string> | null = null

/** Compiles `snapshot.swift` into `binDir` the first time it is needed, and again only when the source changes. */
export function snapshotHelper(binDir: string): Promise<string> {
  helper ??= (async () => {
    const hash = createHash('sha1').update(readFileSync(SOURCE)).digest('hex').slice(0, 12)
    const binary = path.join(binDir, `map-snapshot-${hash}`)
    if (existsSync(binary)) return binary
    await mkdir(binDir, { recursive: true })
    const partial = `${binary}.${process.pid}.part`
    const compile = Bun.spawn(['/usr/bin/swiftc', '-O', '-o', partial, SOURCE], { stdout: 'ignore', stderr: 'pipe' })
    const [code, stderr] = await Promise.all([compile.exited, new Response(compile.stderr).text()])
    if (code !== 0) throw new Error(`maps: swiftc exited ${code}: ${stderr.trim().slice(0, 500)}`)
    await rename(partial, binary)
    return binary
  })().catch((error) => {
    helper = null
    throw error
  })
  return helper
}

export function mapKitRenderer(binDir: string): Renderer {
  return async (request) => {
    const binary = await snapshotHelper(binDir)
    const args = [request.lat, request.lon, request.w, request.h, request.scale, request.dark ? 1 : 0, request.span].map(String)
    const child = Bun.spawn([binary, ...args], { stdout: 'pipe', stderr: 'pipe' })
    const timer = setTimeout(() => child.kill(), RENDER_TIMEOUT_MS)
    try {
      const [code, png, stderr] = await Promise.all([child.exited, new Response(child.stdout).arrayBuffer(), new Response(child.stderr).text()])
      if (code !== 0) throw new Error(`maps: MapKit snapshot failed (${code}): ${stderr.trim().slice(0, 300)}`)
      return new Uint8Array(png)
    } finally {
      clearTimeout(timer)
    }
  }
}

/** `GET /findmy/snapshot`: 400 for a bad request, 503 when MapKit could not render it. */
export async function snapshotResponse(params: URLSearchParams, cache: SnapshotCache): Promise<Response> {
  const request = parseSnapshotRequest(params)
  if ('error' in request) return Response.json({ error: request.error }, { status: 400 })
  try {
    const png = await cache.get(request)
    return new Response(png, { headers: { 'content-type': 'image/png', 'cache-control': `private, max-age=${SNAPSHOT_TTL_MS / 1000}` } })
  } catch (error) {
    return Response.json({ error: error instanceof Error ? error.message : String(error) }, { status: 503 })
  }
}
