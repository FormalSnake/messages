/**
 * Klipy GIF API client, used by the composer's GIF picker.
 *
 * Docs: https://docs.klipy.com/gifs-api, https://docs.klipy.com/gifs-api/gifs-search-api,
 * https://docs.klipy.com/gifs-api/gifs-trending-api, https://docs.klipy.com/attribution
 *
 * The app key comes from https://partner.klipy.com/ (API Keys) and rides in
 * the URL path, not a header. A key in Testing mode is capped at 100
 * requests/hour; requesting Production access in the same panel lifts that.
 * Klipy's attribution guidelines require the search field's placeholder to
 * read "Search KLIPY" (enforced in the desktop picker, not here).
 */

import { stat, writeFile } from 'node:fs/promises'
import path from 'node:path'

export interface Gif {
  id: string
  previewUrl: string
  gifUrl: string
  width: number
  height: number
}

export interface GifPage {
  items: Gif[]
  hasMore: boolean
}

interface KlipyFile {
  url: string
  width: number
  height: number
  size: number
}

/** Klipy serves every item at four sizes; each size may carry gif/webp/mp4 variants. */
interface KlipySize {
  gif?: KlipyFile
}

interface KlipyItem {
  id: number
  slug: string
  title: string
  file: { hd?: KlipySize; md?: KlipySize; sm?: KlipySize; xs?: KlipySize }
}

interface KlipyResponse {
  result: boolean
  data: { data: KlipyItem[]; current_page: number; per_page: number; has_next: boolean }
}

const BASE_URL = 'https://api.klipy.com/api/v1'
const TIMEOUT_MS = 10_000

function toGif(item: KlipyItem): Gif | null {
  const full = item.file.hd?.gif ?? item.file.md?.gif ?? item.file.sm?.gif ?? item.file.xs?.gif
  const preview = item.file.sm?.gif ?? item.file.md?.gif ?? full
  if (!full || !preview) return null
  return { id: String(item.id), previewUrl: preview.url, gifUrl: full.url, width: full.width, height: full.height }
}

export class KlipyClient {
  constructor(private readonly options: { apiKey: string }) {}

  trending(page = 1): Promise<GifPage> {
    return this.fetchPage('trending', { page: String(page) })
  }

  search(query: string, page = 1): Promise<GifPage> {
    return this.fetchPage('search', { page: String(page), q: query })
  }

  private async fetchPage(endpoint: 'trending' | 'search', params: Record<string, string>): Promise<GifPage> {
    const url = new URL(`${BASE_URL}/${this.options.apiKey}/gifs/${endpoint}`)
    for (const [key, value] of Object.entries(params)) url.searchParams.set(key, value)
    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), TIMEOUT_MS)
    try {
      const response = await fetch(url, { signal: controller.signal })
      if (!response.ok) throw new Error(`klipy: ${endpoint} returned ${response.status}`)
      const body = (await response.json()) as KlipyResponse
      if (!body.result) throw new Error(`klipy: ${endpoint} request failed`)
      const items: Gif[] = []
      for (const raw of body.data.data) {
        const gif = toGif(raw)
        if (gif) items.push(gif)
      }
      return { items, hasMore: body.data.has_next }
    } finally {
      clearTimeout(timer)
    }
  }
}

async function downloadTo(url: string, filePath: string): Promise<string> {
  const exists = await stat(filePath).then(
    () => true,
    () => false,
  )
  if (exists) return filePath
  const response = await fetch(url)
  if (!response.ok) throw new Error(`klipy: download returned ${response.status}`)
  await writeFile(filePath, Buffer.from(await response.arrayBuffer()))
  return filePath
}

/** Saves the animated GIF into the attachment cache, ready to send. */
export function downloadGif(gif: Gif, attachmentsDir: string): Promise<string> {
  return downloadTo(gif.gifUrl, path.join(attachmentsDir, `klipy-${gif.id}.gif`))
}

/** Saves the small preview into the attachment cache; the picker's `<img>` cannot load it from http. */
export function downloadGifPreview(gif: Gif, attachmentsDir: string): Promise<string> {
  return downloadTo(gif.previewUrl, path.join(attachmentsDir, `klipy-${gif.id}-preview.gif`))
}
