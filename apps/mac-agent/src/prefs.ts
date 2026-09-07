/**
 * Pinned/muted chat state, shared across every Linux client through this
 * Mac at `~/.config/messages/prefs.json`. `updatedAt` is a last-write-wins
 * clock: two clients editing while offline converge without a lock, as long
 * as every write carries a fresh timestamp.
 */

import { chmod, mkdir, rename } from 'node:fs/promises'
import path from 'node:path'

export interface ChatPrefs {
  pinned?: boolean
  /** When a client last pinned or unpinned; only a pin moves it, so it outranks a Mac pin on its own clock. */
  pinnedAt?: number
  muted?: boolean
  readReceipts?: boolean
  draft?: string
  updatedAt: number
}

interface FavoritedGif {
  id: string
  previewUrl: string
  gifUrl: string
  width: number
  height: number
}

/** A favorited GIF, keyed by gif id. An unfavorite keeps the entry as a `removed` tombstone. */
export interface GifFavorite {
  gif: FavoritedGif
  updatedAt: number
  removed?: boolean
}

export interface PrefsState {
  version: 1
  chats: Record<string, ChatPrefs>
  gifs: Record<string, GifFavorite>
}

function emptyPrefs(): PrefsState {
  return { version: 1, chats: {}, gifs: {} }
}

function prefsPath(configDir: string): string {
  return path.join(configDir, 'prefs.json')
}

/** Whitelists known fields and drops an entry with no usable `updatedAt`, whatever the source. */
function sanitizeEntry(raw: unknown): ChatPrefs | null {
  if (typeof raw !== 'object' || raw === null) return null
  const record = raw as Record<string, unknown>
  if (typeof record.updatedAt !== 'number' || !Number.isFinite(record.updatedAt)) return null
  const entry: ChatPrefs = { updatedAt: record.updatedAt }
  if (typeof record.pinned === 'boolean') entry.pinned = record.pinned
  if (typeof record.pinnedAt === 'number' && Number.isFinite(record.pinnedAt)) entry.pinnedAt = record.pinnedAt
  if (typeof record.muted === 'boolean') entry.muted = record.muted
  if (typeof record.readReceipts === 'boolean') entry.readReceipts = record.readReceipts
  if (typeof record.draft === 'string') entry.draft = record.draft
  return entry
}

/** Whitelists a GIF's own fields; drops the entry if any is missing or the wrong type. */
function sanitizeGif(raw: unknown): FavoritedGif | null {
  if (typeof raw !== 'object' || raw === null) return null
  const record = raw as Record<string, unknown>
  if (typeof record.id !== 'string' || typeof record.previewUrl !== 'string' || typeof record.gifUrl !== 'string') return null
  if (typeof record.width !== 'number' || typeof record.height !== 'number') return null
  return { id: record.id, previewUrl: record.previewUrl, gifUrl: record.gifUrl, width: record.width, height: record.height }
}

/** Same shape as `sanitizeEntry`, for a favorited-GIF entry keyed by gif id. */
function sanitizeGifEntry(raw: unknown): GifFavorite | null {
  if (typeof raw !== 'object' || raw === null) return null
  const record = raw as Record<string, unknown>
  if (typeof record.updatedAt !== 'number' || !Number.isFinite(record.updatedAt)) return null
  const gif = sanitizeGif(record.gif)
  if (!gif) return null
  const entry: GifFavorite = { gif, updatedAt: record.updatedAt }
  if (typeof record.removed === 'boolean') entry.removed = record.removed
  return entry
}

/**
 * Per chat (or gif id), the entry with the newer `updatedAt` wins; an equal
 * timestamp keeps `current`'s entry. `incoming.gifs` is optional so an older
 * client that sends no favorites at all leaves the server's map untouched.
 */
export function mergePrefs(current: PrefsState, incoming: { chats: Record<string, unknown>; gifs?: Record<string, unknown> }): PrefsState {
  const chats: Record<string, ChatPrefs> = { ...current.chats }
  for (const [guid, raw] of Object.entries(incoming.chats)) {
    const entry = sanitizeEntry(raw)
    if (!entry) continue
    const existing = chats[guid]
    if (!existing || entry.updatedAt > existing.updatedAt) chats[guid] = entry
  }
  const gifs: Record<string, GifFavorite> = { ...current.gifs }
  for (const [id, raw] of Object.entries(incoming.gifs ?? {})) {
    const entry = sanitizeGifEntry(raw)
    if (!entry) continue
    const existing = gifs[id]
    if (!existing || entry.updatedAt > existing.updatedAt) gifs[id] = entry
  }
  return { version: 1, chats, gifs }
}

export async function loadPrefs(configDir: string): Promise<PrefsState> {
  try {
    const file = Bun.file(prefsPath(configDir))
    if (!(await file.exists())) return emptyPrefs()
    const parsed = (await file.json()) as { chats?: Record<string, unknown>; gifs?: Record<string, unknown> }
    return mergePrefs(emptyPrefs(), { chats: parsed.chats ?? {}, gifs: parsed.gifs ?? {} })
  } catch (error) {
    console.error(`prefs: ignoring ${prefsPath(configDir)}: ${String(error)}`)
    return emptyPrefs()
  }
}

export async function savePrefs(configDir: string, state: PrefsState): Promise<void> {
  await mkdir(configDir, { recursive: true, mode: 0o700 })
  const file = prefsPath(configDir)
  const temp = `${file}.${process.pid}.tmp`
  await Bun.write(temp, JSON.stringify(state), { mode: 0o600 })
  // Bun.write's `mode` option does not reliably land (observed 0644 on 1.3.13), so chmod explicitly.
  await chmod(temp, 0o600)
  await rename(temp, file)
}

// Chains reads and writes for a given config dir so two racing PUTs don't
// both read the pre-update file and clobber each other's merge.
let writing: Promise<unknown> = Promise.resolve()

/** Reads the current file, merges `incoming` in, saves atomically, and returns the merged state. */
export function updatePrefs(configDir: string, incoming: { chats: Record<string, unknown>; gifs?: Record<string, unknown> }): Promise<PrefsState> {
  const next = writing.then(async () => {
    const merged = mergePrefs(await loadPrefs(configDir), incoming)
    await savePrefs(configDir, merged)
    return merged
  })
  writing = next.catch(() => undefined)
  return next
}
