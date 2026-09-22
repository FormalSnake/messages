import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, test } from 'vitest'
import { cachedFile, fileDigest, shareByContent } from './dedupe'

let dir: string
beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), 'messages-dedupe-'))
})
afterEach(async () => {
  await rm(dir, { recursive: true, force: true })
})

describe('shareByContent', () => {
  test('two entries with the same bytes end up behind one shared file', async () => {
    const a = join(dir, 'guid-a.gif')
    const b = join(dir, 'guid-b.gif')
    await writeFile(a, 'GIF89a same bytes')
    await writeFile(b, 'GIF89a same bytes')
    const sharedA = await shareByContent(a, '.gif')
    const sharedB = await shareByContent(b, '.gif')
    expect(sharedB).toBe(sharedA)
    expect(sharedA).toBe(join(dir, `${await fileDigest(sharedA)}.gif`))
    expect(await readFile(a, 'utf8')).toBe('GIF89a same bytes')
    expect(await readFile(b, 'utf8')).toBe('GIF89a same bytes')
  })

  test('different bytes stay apart', async () => {
    const a = join(dir, 'guid-a.png')
    const b = join(dir, 'guid-b.png')
    await writeFile(a, 'one')
    await writeFile(b, 'two')
    expect(await shareByContent(a, '.png')).not.toBe(await shareByContent(b, '.png'))
  })
})

describe('cachedFile', () => {
  test('resolves a shared entry to its file and a plain entry to itself', async () => {
    const plain = join(dir, 'plain.jpg')
    await writeFile(plain, 'jpeg')
    expect(await cachedFile(plain)).toBe(plain)
    const entry = join(dir, 'guid.jpg')
    await writeFile(entry, 'jpeg')
    const shared = await shareByContent(entry, '.jpg')
    expect(await cachedFile(entry)).toBe(shared)
  })

  test('is null for a missing entry and for a link whose target is gone', async () => {
    expect(await cachedFile(join(dir, 'missing.jpg'))).toBeNull()
    const entry = join(dir, 'guid.jpg')
    await writeFile(entry, 'jpeg')
    const shared = await shareByContent(entry, '.jpg')
    await rm(shared)
    expect(await cachedFile(entry)).toBeNull()
  })
})
