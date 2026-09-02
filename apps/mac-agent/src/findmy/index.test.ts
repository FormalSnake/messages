import { existsSync } from 'node:fs'
import { describe, expect, test } from 'bun:test'
import { FMIP_KEY_PATH, LOCAL_STORAGE_KEY_PATH, fetchDevices, fetchFriends, keyAvailability } from './index'

// This machine has none of the three key files yet (they get produced on the
// owner's Mac and copied over separately), so these exercise the "no keys"
// path every fresh install starts from.
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
