/**
 * Orchestrates the three Find My key files into the two endpoints `agent.ts`
 * serves: friends' live locations (via `LocalStorage.db`, AES page cipher)
 * and devices (via `Devices.data`, the ChaCha20-Poly1305 cache envelope).
 */

import { existsSync } from 'node:fs'
import { homedir } from 'node:os'
import path from 'node:path'
import { decryptCacheFile, loadRawKey, loadSymmetricKey } from './cache'
import { candidateDbPaths, readFriendLocations } from './localstorage'
import { asNumber, asRecord, asString, type PlistValue } from './plist'

const home = homedir()

const KEYS_DIR = path.join(home, '.config', 'messages', 'findmy')
export const LOCAL_STORAGE_KEY_PATH = path.join(KEYS_DIR, 'LocalStorage.key')
export const FMF_KEY_PATH = path.join(KEYS_DIR, 'FMFDataManager.bplist')
export const FMIP_KEY_PATH = path.join(KEYS_DIR, 'FMIPDataManager.bplist')

export const DEVICES_DATA_PATH = path.join(home, 'Library', 'Caches', 'com.apple.findmy.fmipcore', 'Devices.data')
export const FRIEND_CACHE_DATA_PATH = path.join(home, 'Library', 'Caches', 'com.apple.findmy.fmfcore', 'FriendCacheData.data')

export const SQLITE_CACHE_PATH = path.join(home, 'Library', 'Caches', 'messages', 'findmy', 'LocalStorage.sqlite')

export interface KeyAvailability {
  friends: boolean
  fmf: boolean
  fmip: boolean
}

export function keyAvailability(): KeyAvailability {
  return {
    friends: existsSync(LOCAL_STORAGE_KEY_PATH),
    fmf: existsSync(FMF_KEY_PATH),
    fmip: existsSync(FMIP_KEY_PATH),
  }
}

export interface FriendLocation {
  id: string
  name?: string
  addresses: string[]
  latitude: number
  longitude: number
  accuracy?: number
  timestamp: number
  isSharing: boolean
}

export async function fetchFriends(): Promise<FriendLocation[]> {
  const key = loadRawKey(LOCAL_STORAGE_KEY_PATH)
  if (!key) throw new Error(`findmy: ${LOCAL_STORAGE_KEY_PATH} is missing or not 32 bytes`)
  return readFriendLocations(candidateDbPaths(home), key, SQLITE_CACHE_PATH)
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

/**
 * `location` wins when present (it is the fresher of the two); a
 * `crowdSourcedLocation` Find My network sighting rescues the record only
 * when Apple flags it not old, matching `CacheDecryptor.positionSource`.
 */
function positionSource(device: Record<string, PlistValue>): Record<string, PlistValue> | null {
  const primary = asRecord(device.location)
  if (primary && asNumber(primary.latitude) !== undefined) return primary
  const sighting = asRecord(device.crowdSourcedLocation)
  if (sighting && sighting.isOld === false) return sighting
  return null
}

export function fetchDevices(): DeviceLocation[] {
  const key = loadSymmetricKey(FMIP_KEY_PATH)
  if (!key) throw new Error(`findmy: ${FMIP_KEY_PATH} is missing or has no symmetricKey`)
  const payload = decryptCacheFile(DEVICES_DATA_PATH, key)
  if (!Array.isArray(payload)) return []

  const devices: DeviceLocation[] = []
  for (const item of payload) {
    const device = asRecord(item)
    if (!device) continue
    const id = asString(device.baUUID) ?? asString(device.deviceDiscoveryId) ?? asString(device.identifier) ?? asString(device.serialNumber)
    if (!id) continue
    const location = positionSource(device)
    const latitude = asNumber(location?.latitude)
    const longitude = asNumber(location?.longitude)
    if (!location || latitude === undefined || longitude === undefined) continue
    devices.push({
      id,
      name: asString(device.name) ?? id,
      latitude,
      longitude,
      accuracy: asNumber(location.horizontalAccuracy),
      battery: asNumber(device.batteryLevel),
      // Already unix ms on this cache, unlike `secureLocations.value.timestamp`'s Apple-epoch seconds.
      timestamp: asNumber(location.timeStamp),
    })
  }
  return devices
}

interface CacheEntry<T> {
  value: T
  at: number
}

const CACHE_MS = 30_000
let friendsCache: CacheEntry<FriendLocation[]> | null = null
let devicesCache: CacheEntry<DeviceLocation[]> | null = null

export async function cachedFriends(): Promise<{ friends: FriendLocation[]; updatedAt: number }> {
  const now = Date.now()
  if (!friendsCache || now - friendsCache.at > CACHE_MS) friendsCache = { value: await fetchFriends(), at: now }
  return { friends: friendsCache.value, updatedAt: friendsCache.at }
}

export function cachedDevices(): { devices: DeviceLocation[]; updatedAt: number } {
  const now = Date.now()
  if (!devicesCache || now - devicesCache.at > CACHE_MS) devicesCache = { value: fetchDevices(), at: now }
  return { devices: devicesCache.value, updatedAt: devicesCache.at }
}

/** Test-only: drops both caches so a test does not observe another test's result. */
export function resetCachesForTesting(): void {
  friendsCache = null
  devicesCache = null
}
