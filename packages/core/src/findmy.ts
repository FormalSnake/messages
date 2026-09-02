/**
 * Client for the `@messages/mac-agent` HTTP server: the Mac decrypts Find My's
 * on-disk caches (see that package's `src/findmy/`) and this talks to it over
 * the network, the same way `bluebubbles/client.ts` talks to BlueBubbles.
 */

import { mkdir, stat } from 'node:fs/promises'
import path from 'node:path'

export interface FriendLocation {
  id: string
  name?: string
  addresses: string[]
  latitude: number
  longitude: number
  accuracy?: number
  timestamp: number
  label?: string
  isSharing: boolean
}

export interface DeviceLocation {
  id: string
  name: string
  latitude: number
  longitude: number
  accuracy?: number
  /** 0-1 charge level, when Apple reports one. */
  battery?: number
  timestamp?: number
}

export interface FindMyHealth {
  ok: boolean
  keys: { friends: boolean; fmf: boolean; fmip: boolean }
}

export interface FindMyClientOptions {
  url: string
  token: string
}

const TIMEOUT_MS = 10_000

async function fetchJson<T>(url: string, init: RequestInit = {}): Promise<T> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), TIMEOUT_MS)
  try {
    const response = await fetch(url, { ...init, signal: controller.signal })
    if (!response.ok) throw new Error(`findmy: ${url} returned ${response.status}`)
    return (await response.json()) as T
  } finally {
    clearTimeout(timer)
  }
}

export class FindMyClient {
  constructor(private readonly options: FindMyClientOptions) {}

  private endpoint(pathname: string): string {
    return new URL(pathname, this.options.url).toString()
  }

  private authHeaders(): HeadersInit {
    return { authorization: `Bearer ${this.options.token}` }
  }

  health(): Promise<FindMyHealth> {
    return fetchJson<FindMyHealth>(this.endpoint('/health'))
  }

  friends(): Promise<{ friends: FriendLocation[]; updatedAt: number }> {
    return fetchJson(this.endpoint('/findmy/friends'), { headers: this.authHeaders() })
  }

  devices(): Promise<{ devices: DeviceLocation[]; updatedAt: number }> {
    return fetchJson(this.endpoint('/findmy/devices'), { headers: this.authHeaders() })
  }
}

function digitsOf(address: string): string {
  return address.replace(/\D/g, '')
}

/** Emails compare case-insensitively; phone numbers compare on their last 9 digits, so a local number and its +country-code form still match. */
export function normalizeAddress(address: string): string {
  if (address.includes('@')) return address.toLowerCase()
  const digits = digitsOf(address)
  return digits.length >= 9 ? digits.slice(-9) : digits
}

/** Every address of the contact that owns `address`, so a chat on someone's email still finds their phone in Find My. */
export function contactAddresses(contacts: Array<{ addresses: string[] }>, address: string): string[] {
  const wanted = normalizeAddress(address)
  const owner = contacts.find((contact) => contact.addresses.some((item) => normalizeAddress(item) === wanted))
  return owner ? owner.addresses : []
}

/** Matches a chat participant's addresses against Find My friends. */
export function matchFriend(friends: FriendLocation[], addresses: string[]): FriendLocation | undefined {
  const wanted = new Set(addresses.map(normalizeAddress))
  return friends.find((friend) => friend.addresses.some((address) => wanted.has(normalizeAddress(address))))
}

const TILE_SIZE = 256
const DAY_MS = 24 * 60 * 60 * 1000

/**
 * Downloads (or reuses a cached) OpenStreetMap tile covering `lat`/`lon` and
 * returns its local path plus the point's pixel offset inside that tile, so
 * a `LocationCard` can place a pin without its own web-mercator math.
 */
export async function tileFor(lat: number, lon: number, zoom: number, cacheDir: string): Promise<{ path: string; px: number; py: number }> {
  const scale = 2 ** zoom
  const xFraction = ((lon + 180) / 360) * scale
  const latRad = (lat * Math.PI) / 180
  const yFraction = ((1 - Math.log(Math.tan(latRad) + 1 / Math.cos(latRad)) / Math.PI) / 2) * scale
  const x = Math.floor(xFraction)
  const y = Math.floor(yFraction)
  const px = Math.floor((xFraction - x) * TILE_SIZE)
  const py = Math.floor((yFraction - y) * TILE_SIZE)

  const tilesDir = path.join(cacheDir, 'tiles')
  await mkdir(tilesDir, { recursive: true })
  const tilePath = path.join(tilesDir, `${zoom}-${x}-${y}.png`)

  const stats = await stat(tilePath).catch(() => null)
  if (!stats || Date.now() - stats.mtimeMs > DAY_MS) {
    const response = await fetch(`https://tile.openstreetmap.org/${zoom}/${x}/${y}.png`, {
      headers: { 'User-Agent': 'messages-linux/0.1 (github.com/FormalSnake/messages)' },
    })
    if (!response.ok) throw new Error(`findmy: tile fetch for ${zoom}/${x}/${y} returned ${response.status}`)
    await Bun.write(tilePath, await response.arrayBuffer())
  }

  return { path: tilePath, px, py }
}
