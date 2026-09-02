/**
 * The FindMy `.data` caches (`FriendCacheData.data`, `Devices.data`, ...):
 * a binary plist `{ signature, encryptedData }` where `encryptedData` is a
 * 12-byte nonce, ChaCha20-Poly1305 ciphertext, and a 16-byte tag, sealed
 * with no associated data. Confirmed against
 * manonstreet/FindMySyncPlus's `CacheDecryptor.swift`.
 */

import { readFileSync } from 'node:fs'
import { chacha20Poly1305Decrypt } from './chacha'
import { asRecord, parseBinaryPlist, parsePayload, type PlistValue } from './plist'

/**
 * The `FMFDataManager.bplist` / `FMIPDataManager.bplist` key files nest their
 * 32-byte symmetric key under `symmetricKey`, sometimes as raw bytes,
 * sometimes as `symmetricKey.key.data` (a keyed-archiver style wrapper).
 * Recursing through `symmetricKey` / `key` / `data` and accepting a base64
 * string covers both shapes, matching `CacheDecryptor.extractSymmetricKey`.
 */
export function extractSymmetricKey(value: PlistValue): Uint8Array | null {
  if (value instanceof Uint8Array) return value.length === 32 ? value : null
  if (typeof value === 'string') {
    try {
      const decoded = Uint8Array.from(atob(value), (char) => char.charCodeAt(0))
      return decoded.length === 32 ? decoded : null
    } catch {
      return null
    }
  }
  const record = asRecord(value)
  if (!record) return null
  for (const field of ['symmetricKey', 'key', 'data']) {
    if (field in record) {
      const nested = extractSymmetricKey(record[field]!)
      if (nested) return nested
    }
  }
  return null
}

/** Reads a key `.bplist` (`FMFDataManager.bplist` / `FMIPDataManager.bplist`) and returns its 32-byte symmetric key. */
export function loadSymmetricKey(bplistPath: string): Uint8Array | null {
  const bytes = new Uint8Array(readFileSync(bplistPath))
  const plist = parseBinaryPlist(bytes)
  return extractSymmetricKey(plist)
}

/** Reads a raw `LocalStorage.key` file (32 bytes, no wrapping). */
export function loadRawKey(keyPath: string): Uint8Array | null {
  const bytes = new Uint8Array(readFileSync(keyPath))
  return bytes.length === 32 ? bytes : null
}

/** Decrypts one `.data` cache file (`FriendCacheData.data`, `Devices.data`, `Items.data`, ...) and parses its plaintext plist or JSON. */
export function decryptCacheFile(dataPath: string, key: Uint8Array): PlistValue {
  const bytes = new Uint8Array(readFileSync(dataPath))
  const outer = asRecord(parseBinaryPlist(bytes))
  const encrypted = outer?.encryptedData
  if (!(encrypted instanceof Uint8Array)) throw new Error(`findmy: ${dataPath} has no encryptedData`)
  if (encrypted.length < 12 + 16) throw new Error(`findmy: ${dataPath}'s encryptedData is shorter than nonce + tag`)
  const nonce = encrypted.subarray(0, 12)
  const sealed = encrypted.subarray(12)
  const plaintext = chacha20Poly1305Decrypt(key, nonce, sealed)
  return parsePayload(plaintext)
}
