/**
 * Pushes a snapshot the moment Find My writes one, for `/findmy/stream`. The
 * source files are watched rather than polled, so a location reaches a client
 * about as fast as it reaches the Mac. The watchers only run while someone is
 * subscribed.
 */

import { watch, type FSWatcher } from 'node:fs'
import path from 'node:path'
import { cachedDevices, cachedFriends, deviceSourcePaths, friendSourcePaths, type DeviceLocation, type FriendLocation } from './index'

export interface FindMySnapshot {
  /** null when that half could not be read (a missing key, mostly), so a client keeps what it has rather than blanking the map. */
  friends: FriendLocation[] | null
  devices: DeviceLocation[] | null
  updatedAt: number
}

/** Find My rewrites the friend store in bursts; one read per burst is enough. */
const DEBOUNCE_MS = 250
/** Backstop for a write the watcher missed. Find My writes every 20 seconds or so, and an unchanged read costs nothing. */
const SWEEP_MS = 15_000

type Listener = (snapshot: FindMySnapshot) => void

const listeners = new Set<Listener>()
let watchers: FSWatcher[] = []
let sweep: ReturnType<typeof setInterval> | null = null
let debounce: ReturnType<typeof setTimeout> | null = null
let lastSent = ''
let publishing = false
let pending = false

async function snapshot(): Promise<FindMySnapshot> {
  const friends = await cachedFriends().then((result) => result.friends, () => null)
  let devices: DeviceLocation[] | null = null
  try {
    devices = cachedDevices().devices
  } catch {
    devices = null
  }
  return { friends, devices, updatedAt: Date.now() }
}

function payloadOf(snap: FindMySnapshot): string {
  return JSON.stringify({ friends: snap.friends, devices: snap.devices })
}

/** Reads the sources and tells everyone, but only when the decoded payload actually differs from the last one sent. */
async function publish(): Promise<void> {
  if (publishing) {
    pending = true
    return
  }
  publishing = true
  try {
    const next = await snapshot()
    const payload = payloadOf(next)
    if (payload !== lastSent) {
      lastSent = payload
      for (const listener of listeners) listener(next)
    }
  } catch (error) {
    console.error(`findmy: feed read failed: ${String(error)}`)
  } finally {
    publishing = false
  }
  if (pending) {
    pending = false
    await publish()
  }
}

function schedule(): void {
  if (debounce) clearTimeout(debounce)
  debounce = setTimeout(() => {
    debounce = null
    void publish()
  }, DEBOUNCE_MS)
}

function start(): void {
  const dirs = new Set([...friendSourcePaths(), ...deviceSourcePaths()].map((file) => path.dirname(file)))
  for (const dir of dirs) {
    try {
      watchers.push(watch(dir, () => schedule()))
    } catch {
      // A cache directory Find My has not created yet. The sweep picks it up.
    }
  }
  sweep = setInterval(() => void publish(), SWEEP_MS)
}

function stop(): void {
  for (const watcher of watchers) watcher.close()
  watchers = []
  if (sweep) clearInterval(sweep)
  sweep = null
  if (debounce) clearTimeout(debounce)
  debounce = null
  lastSent = ''
}

/** Sends the current snapshot straight away, so a client paints without waiting for Find My's next write. */
async function deliverCurrent(listener: Listener): Promise<void> {
  const current = await snapshot()
  lastSent = payloadOf(current)
  if (listeners.has(listener)) listener(current)
}

export function subscribeFindMy(listener: Listener): () => void {
  listeners.add(listener)
  if (listeners.size === 1) start()
  void deliverCurrent(listener)
  return () => {
    listeners.delete(listener)
    if (listeners.size === 0) stop()
  }
}
