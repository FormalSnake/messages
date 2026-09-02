import { existsSync, mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { describe, expect, test } from 'bun:test'

// Point the module at an empty key directory so these exercise the "no keys"
// path every fresh install starts from, whatever this machine has installed.
process.env.MESSAGES_FINDMY_KEYS_DIR = mkdtempSync(path.join(tmpdir(), 'findmy-keys-'))
const { FMIP_KEY_PATH, LOCAL_STORAGE_KEY_PATH, fetchDevices, fetchFriends, keyAvailability } = await import('./index')

describe('keyAvailability', () => {
  test('reports false for every key file that is not present', () => {
    expect(existsSync(LOCAL_STORAGE_KEY_PATH)).toBe(false)
    expect(keyAvailability()).toEqual({ friends: false, fmf: false, fmip: false })
  })
})

describe('fetchFriends', () => {
  test('throws a message naming the missing key file, not a raw ENOENT', async () => {
    await expect(fetchFriends()).rejects.toThrow(/LocalStorage\.key/)
  })
})

describe('fetchDevices', () => {
  test('throws a message naming the missing key file, not a raw ENOENT', () => {
    expect(() => fetchDevices()).toThrow(/FMIPDataManager\.bplist/)
    expect(FMIP_KEY_PATH.endsWith('FMIPDataManager.bplist')).toBe(true)
  })
})
