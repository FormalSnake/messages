import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { describe, expect, test } from 'vitest'
import { encode as encodeJpeg } from 'jpeg-js'
import { pngFor } from './clipboard'
import { parseUriList } from './clipboard'

const existing = new URL(import.meta.url).pathname // this test file itself, guaranteed to exist

describe('parseUriList', () => {
  test('decodes file:// entries that exist', () => {
    const text = `file://${encodeURI(existing)}\n`
    expect(parseUriList(text)).toEqual([existing])
  })

  test('drops entries that are not file:// uris', () => {
    expect(parseUriList('https://example.com/a.jpg\n')).toEqual([])
  })

  test('drops comments and blank lines', () => {
    const text = `# a comment\n\nfile://${encodeURI(existing)}\n`
    expect(parseUriList(text)).toEqual([existing])
  })

  test('drops file:// entries whose path does not exist', () => {
    expect(parseUriList('file:///no/such/file-xyz.jpg\n')).toEqual([])
  })
})

describe('pngFor', () => {
  test('hands a png straight back', async () => {
    expect(await pngFor('/some/picture.png', 'image/png')).toBe('/some/picture.png')
  })

  test('re-encodes a jpeg as png once', async () => {
    const dir = await mkdtemp(path.join(tmpdir(), 'messages-clip-'))
    const source = path.join(dir, 'photo.jpg')
    const data = Buffer.alloc(4 * 4 * 4, 0x80)
    await writeFile(source, encodeJpeg({ data, width: 4, height: 4 }, 90).data)
    const png = await pngFor(source, 'image/jpeg')
    expect(png).toMatch(/clip-photo\.png$/)
    const bytes = await readFile(png!)
    expect(Array.from(bytes.slice(0, 4))).toEqual([0x89, 0x50, 0x4e, 0x47])
    await rm(dir, { recursive: true, force: true })
  })

  test('leaves formats it cannot decode alone', async () => {
    expect(await pngFor('/some/clip.gif', 'image/gif')).toBeNull()
  })
})
