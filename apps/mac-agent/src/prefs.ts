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
  muted?: boolean
  readReceipts?: boolean
  draft?: string
  updatedAt: number
}

export interface PrefsState {
  version: 1
  chats: Record<string, ChatPrefs>
}

function emptyPrefs(): PrefsState {
  return { version: 1, chats: {} }
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
  if (typeof record.muted === 'boolean') entry.muted = record.muted
  if (typeof record.readReceipts === 'boolean') entry.readReceipts = record.readReceipts
  if (typeof record.draft === 'string') entry.draft = record.draft
  return entry
}

/** Per chat, the entry with the newer `updatedAt` wins; an equal timestamp keeps `current`'s entry. */
export function mergePrefs(current: PrefsState, incoming: { chats: Record<string, unknown> }): PrefsState {
  const chats: Record<string, ChatPrefs> = { ...current.chats }
  for (const [guid, raw] of Object.entries(incoming.chats)) {
    const entry = sanitizeEntry(raw)
    if (!entry) continue
    const existing = chats[guid]
    if (!existing || entry.updatedAt > existing.updatedAt) chats[guid] = entry
  }
  return { version: 1, chats }
}

export async function loadPrefs(configDir: string): Promise<PrefsState> {
  try {
    const file = Bun.file(prefsPath(configDir))
    if (!(await file.exists())) return emptyPrefs()
    const parsed = (await file.json()) as { chats?: Record<string, unknown> }
    return mergePrefs(emptyPrefs(), { chats: parsed.chats ?? {} })
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
export function updatePrefs(configDir: string, incoming: { chats: Record<string, unknown> }): Promise<PrefsState> {
  const next = writing.then(async () => {
    const merged = mergePrefs(await loadPrefs(configDir), incoming)
    await savePrefs(configDir, merged)
    return merged
  })
  writing = next.catch(() => undefined)
  return next
}
