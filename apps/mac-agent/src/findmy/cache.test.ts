import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, test } from 'bun:test'
import { decryptCacheFile, extractSymmetricKey, loadSymmetricKey } from './cache'
import { asRecord, parseBinaryPlist } from './plist'

// `key-flat.plist`, `key-nested.plist` and `devices-cache.plist` are real
// bplists (`plutil -convert binary1`) sealed with the same 32-byte key,
// generated once and checked in as fixtures; see devices-cache-key.txt for
// the plaintext key this was built against.
const FIXTURE_DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../desktop/tmp')

function fixturePath(name: string): string {
  return path.join(FIXTURE_DIR, name)
}

const EXPECTED_KEY_B64 = readFileSync(fixturePath('devices-cache-key.txt'), 'utf8').trim().replace('KEY_B64=', '')

describe('extractSymmetricKey', () => {
  test('reads a raw 32-byte key stored directly under symmetricKey', () => {
    const plist = parseBinaryPlist(new Uint8Array(readFileSync(fixturePath('key-flat.plist'))))
    const key = extractSymmetricKey(plist)
    expect(key).not.toBeNull()
    expect(Buffer.from(key!).toString('base64')).toBe(EXPECTED_KEY_B64)
  })

  test('reads a key nested under symmetricKey.key.data', () => {
    const plist = parseBinaryPlist(new Uint8Array(readFileSync(fixturePath('key-nested.plist'))))
    const key = extractSymmetricKey(plist)
    expect(key).not.toBeNull()
    expect(Buffer.from(key!).toString('base64')).toBe(EXPECTED_KEY_B64)
  })

  test('reads a base64-encoded string key', () => {
    const key = extractSymmetricKey(EXPECTED_KEY_B64)
    expect(key).not.toBeNull()
    expect(Buffer.from(key!).toString('base64')).toBe(EXPECTED_KEY_B64)
  })

  test('rejects a key of the wrong length', () => {
    expect(extractSymmetricKey(new Uint8Array(16))).toBeNull()
  })

  test('rejects a plist with no symmetricKey at all', () => {
    expect(extractSymmetricKey({ somethingElse: 1 })).toBeNull()
  })
})

describe('loadSymmetricKey', () => {
  test('loads the flat form from a bplist file', () => {
    const key = loadSymmetricKey(fixturePath('key-flat.plist'))
    expect(key).not.toBeNull()
    expect(key!.length).toBe(32)
  })
})

describe('decryptCacheFile', () => {
  test('decrypts a real {signature, encryptedData} bplist and parses the JSON plaintext', () => {
    const key = loadSymmetricKey(fixturePath('key-flat.plist'))!
    const payload = decryptCacheFile(fixturePath('devices-cache.plist'), key)
    expect(Array.isArray(payload)).toBe(true)
    const devices = payload as unknown[]
    expect(devices).toHaveLength(2)
    const first = asRecord(devices[0] as never)
    expect(first!.baUUID).toBe('AAAA-1111')
    expect(first!.name).toBe('AirPods Pro')
    const location = asRecord(first!.location as never)
    expect(location!.latitude).toBeCloseTo(37.7749, 4)
    expect(location!.longitude).toBeCloseTo(-122.4194, 4)
  })

  test('throws when the key is wrong (authentication failure)', () => {
    const wrongKey = new Uint8Array(32).fill(7)
    expect(() => decryptCacheFile(fixturePath('devices-cache.plist'), wrongKey)).toThrow(/authentication failed/)
  })
})
