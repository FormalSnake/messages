import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { downloadGif, downloadGifPreview, KlipyClient } from './gifs'

function klipyItem(id: number, overrides: Partial<{ hd: string; sm: string }> = {}) {
  const file = (url: string) => ({ gif: { url, width: 200, height: 200, size: 1024 } })
  return {
    id,
    slug: `slug-${id}`,
    title: `Gif ${id}`,
    file: {
      hd: file(overrides.hd ?? `https://static.klipy.com/${id}/hd.gif`),
      md: file(`https://static.klipy.com/${id}/md.gif`),
      sm: file(overrides.sm ?? `https://static.klipy.com/${id}/sm.gif`),
      xs: file(`https://static.klipy.com/${id}/xs.gif`),
    },
  }
}

function klipyResponse(items: Array<{ id: number; slug: string; title: string; file: Record<string, unknown> }>, hasNext = false) {
  return { result: true, data: { data: items, current_page: 1, per_page: 24, has_next: hasNext } }
}

function jsonResponse(body: unknown, ok = true, status = 200): Response {
  return { ok, status, json: async () => body } as Response
}

describe('KlipyClient', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('trending fetches the trending endpoint with the app key in the path', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(klipyResponse([klipyItem(1)])))
    vi.stubGlobal('fetch', fetchMock)

    const client = new KlipyClient({ apiKey: 'app-key' })
    const page = await client.trending(2)

    const url = new URL(String(fetchMock.mock.calls[0]?.[0]))
    expect(url.pathname).toBe('/api/v1/app-key/gifs/trending')
    expect(url.searchParams.get('page')).toBe('2')
    expect(page.hasMore).toBe(false)
    expect(page.items).toEqual([{ id: '1', previewUrl: 'https://static.klipy.com/1/sm.gif', gifUrl: 'https://static.klipy.com/1/hd.gif', width: 200, height: 200 }])
  })

  it('search sends the query string and reports hasMore', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(klipyResponse([klipyItem(1), klipyItem(2)], true)))
    vi.stubGlobal('fetch', fetchMock)

    const client = new KlipyClient({ apiKey: 'app-key' })
    const page = await client.search('cats', 1)

    const url = new URL(String(fetchMock.mock.calls[0]?.[0]))
    expect(url.pathname).toBe('/api/v1/app-key/gifs/search')
    expect(url.searchParams.get('q')).toBe('cats')
    expect(page.items).toHaveLength(2)
    expect(page.hasMore).toBe(true)
  })

  it('skips an item with no gif file instead of failing the whole page', async () => {
    const broken = { id: 3, slug: 'broken', title: 'Broken', file: {} }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse(klipyResponse([klipyItem(1), broken]))))

    const page = await new KlipyClient({ apiKey: 'app-key' }).trending()
    expect(page.items.map((item) => item.id)).toEqual(['1'])
  })

  it('throws when the server answers with an error status', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse({}, false, 429)))
    await expect(new KlipyClient({ apiKey: 'app-key' }).trending()).rejects.toThrow('429')
  })

  it('throws when result is false', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse({ result: false, data: { data: [], current_page: 1, per_page: 24, has_next: false } })))
    await expect(new KlipyClient({ apiKey: 'app-key' }).trending()).rejects.toThrow('klipy')
  })
})

describe('downloadGif / downloadGifPreview', () => {
  let dir: string

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'messages-klipy-'))
  })

  afterEach(async () => {
    vi.unstubAllGlobals()
    await rm(dir, { recursive: true, force: true })
  })

  const gif = { id: '42', previewUrl: 'https://static.klipy.com/42/sm.gif', gifUrl: 'https://static.klipy.com/42/hd.gif', width: 100, height: 100 }

  it('downloads the animated file into the attachment cache', async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, arrayBuffer: async () => new TextEncoder().encode('gif-bytes').buffer })
    vi.stubGlobal('fetch', fetchMock)

    const saved = await downloadGif(gif, dir)
    expect(saved).toBe(path.join(dir, 'klipy-42.gif'))
    expect(await readFile(saved, 'utf8')).toBe('gif-bytes')
    expect(fetchMock).toHaveBeenCalledWith(gif.gifUrl)
  })

  it('downloads the preview separately from the full file', async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, arrayBuffer: async () => new TextEncoder().encode('preview-bytes').buffer })
    vi.stubGlobal('fetch', fetchMock)

    const saved = await downloadGifPreview(gif, dir)
    expect(saved).toBe(path.join(dir, 'klipy-42-preview.gif'))
    expect(fetchMock).toHaveBeenCalledWith(gif.previewUrl)
  })

  it('keeps the file already on disk instead of downloading again', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    await writeFile(path.join(dir, 'klipy-42.gif'), 'already here')

    const saved = await downloadGif(gif, dir)
    expect(saved).toBe(path.join(dir, 'klipy-42.gif'))
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('throws when the download response is not ok', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 500 }))
    await expect(downloadGif(gif, dir)).rejects.toThrow('500')
  })
})
