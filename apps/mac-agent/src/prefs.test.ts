import { mkdtempSync, readFileSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { describe, expect, test } from 'bun:test'
import { loadPrefs, mergePrefs, savePrefs, updatePrefs } from './prefs'

function tempConfigDir(): string {
  return mkdtempSync(path.join(tmpdir(), 'prefs-'))
}

describe('mergePrefs', () => {
  test('a newer updatedAt wins', () => {
    const current = { version: 1 as const, chats: { a: { pinned: true, updatedAt: 100 } } }
    const merged = mergePrefs(current, { chats: { a: { pinned: false, updatedAt: 200 } } })
    expect(merged.chats.a).toEqual({ pinned: false, updatedAt: 200 })
  })

  test('an equal updatedAt keeps the current entry', () => {
    const current = { version: 1 as const, chats: { a: { pinned: true, updatedAt: 100 } } }
    const merged = mergePrefs(current, { chats: { a: { pinned: false, updatedAt: 100 } } })
    expect(merged.chats.a).toEqual({ pinned: true, updatedAt: 100 })
  })

  test('an incoming entry with no updatedAt is ignored', () => {
    const current = { version: 1 as const, chats: { a: { pinned: true, updatedAt: 100 } } }
    const merged = mergePrefs(current, { chats: { a: { pinned: false }, b: { muted: true } } })
    expect(merged.chats).toEqual({ a: { pinned: true, updatedAt: 100 } })
  })

  test('drops unknown fields even on a winning entry', () => {
    const current = { version: 1 as const, chats: {} }
    const merged = mergePrefs(current, { chats: { a: { pinned: true, updatedAt: 100, evil: 'payload' } } })
    expect(merged.chats.a).toEqual({ pinned: true, updatedAt: 100 })
  })

  test('keeps readReceipts and draft, the same as pinned and muted', () => {
    const current = { version: 1 as const, chats: {} }
    const merged = mergePrefs(current, { chats: { a: { readReceipts: false, draft: 'still typing', updatedAt: 100 } } })
    expect(merged.chats.a).toEqual({ readReceipts: false, draft: 'still typing', updatedAt: 100 })
  })

  test('drops readReceipts and draft when they are the wrong type', () => {
    const current = { version: 1 as const, chats: {} }
    const merged = mergePrefs(current, { chats: { a: { readReceipts: 'off', draft: 42, updatedAt: 100 } } })
    expect(merged.chats.a).toEqual({ updatedAt: 100 })
  })

  test('a chat absent from current is added from incoming', () => {
    const merged = mergePrefs({ version: 1, chats: {} }, { chats: { a: { muted: true, updatedAt: 5 } } })
    expect(merged.chats.a).toEqual({ muted: true, updatedAt: 5 })
  })

  test('a non-object entry is ignored', () => {
    const merged = mergePrefs({ version: 1, chats: {} }, { chats: { a: 'not an object', b: null, c: 42 } })
    expect(merged.chats).toEqual({})
  })
})

describe('loadPrefs', () => {
  test('returns an empty store when the file does not exist', async () => {
    expect(await loadPrefs(tempConfigDir())).toEqual({ version: 1, chats: {} })
  })

  test('returns an empty store when the file is corrupt JSON', async () => {
    const dir = tempConfigDir()
    await Bun.write(path.join(dir, 'prefs.json'), 'not json{')
    expect(await loadPrefs(dir)).toEqual({ version: 1, chats: {} })
  })

  test('sanitizes what it reads back off disk', async () => {
    const dir = tempConfigDir()
    await Bun.write(path.join(dir, 'prefs.json'), JSON.stringify({ version: 1, chats: { a: { pinned: true, updatedAt: 10, junk: 1 } } }))
    expect(await loadPrefs(dir)).toEqual({ version: 1, chats: { a: { pinned: true, updatedAt: 10 } } })
  })
})

describe('savePrefs', () => {
  test('writes atomically with the config dir and file at the expected modes, readable back', async () => {
    const dir = tempConfigDir()
    const nested = path.join(dir, 'nested')
    const state = { version: 1 as const, chats: { a: { pinned: true, updatedAt: 1 } } }
    await savePrefs(nested, state)

    expect(statSync(nested).mode & 0o777).toBe(0o700)
    const filePath = path.join(nested, 'prefs.json')
    expect(statSync(filePath).mode & 0o777).toBe(0o600)
    expect(JSON.parse(readFileSync(filePath, 'utf8'))).toEqual(state)
    expect(await loadPrefs(nested)).toEqual(state)
  })
})

describe('updatePrefs', () => {
  test('merges into and persists the on-disk state', async () => {
    const dir = tempConfigDir()
    await updatePrefs(dir, { chats: { a: { pinned: true, updatedAt: 1 } } })
    const merged = await updatePrefs(dir, { chats: { a: { pinned: false, updatedAt: 2 }, b: { muted: true, updatedAt: 1 } } })
    expect(merged.chats).toEqual({ a: { pinned: false, updatedAt: 2 }, b: { muted: true, updatedAt: 1 } })
    expect(await loadPrefs(dir)).toEqual(merged)
  })

  test('serializes two concurrent updates so the second sees the first already merged in', async () => {
    const dir = tempConfigDir()
    // updatePrefs queues onto its chain synchronously, before either call's first await,
    // so the two calls join the chain in this order regardless of when they settle.
    const [first, second] = await Promise.all([updatePrefs(dir, { chats: { a: { pinned: true, updatedAt: 1 } } }), updatePrefs(dir, { chats: { b: { muted: true, updatedAt: 1 } } })])
    expect(first.chats).toEqual({ a: { pinned: true, updatedAt: 1 } })
    expect(second.chats).toEqual({ a: { pinned: true, updatedAt: 1 }, b: { muted: true, updatedAt: 1 } })
    expect(await loadPrefs(dir)).toEqual(second)
  })
})
