/**
 * `LocalStorage.db`, findmylocateagent's live friend-location store. It is
 * SQLite behind Apple's sqliteCodecCCCrypto page cipher, not the FindMy
 * ChaCha20-Poly1305 envelope the `.data` caches use (see `cache.ts`).
 *
 * Page cipher, confirmed against manonstreet/FindMySyncPlus's
 * `LocalStorageDecryptor.swift`: for page index i (4096-byte pages),
 * pgno = i+1, reserved = page[4084:4096], iv = pgno as little-endian uint32
 * plus the reserved bytes, keystream = AES-256-CBC encrypt of 4096 zero
 * bytes with (key, iv), plaintext = page[0:4084] XOR keystream[0:4084]
 * followed by the reserved bytes verbatim. Page 0 additionally has bytes
 * 16..24 (the SQLite header's page-size and format fields) copied verbatim
 * from the encrypted page: Apple's codec never encrypts them, so XORing
 * them corrupts them.
 */

import { createCipheriv } from 'node:crypto'
import { existsSync, readFileSync, statSync } from 'node:fs'
import { chmod, mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { Database } from 'bun:sqlite'
import { asNumber, asRecord, parseBinaryPlist, type PlistValue } from './plist'

export const PAGE_SIZE = 4096
const RESERVED_OFFSET = 4084
const RESERVED_SIZE = 12
const SQLITE_MAGIC = new TextEncoder().encode('SQLite format 3\0')

function equalPrefix(bytes: Uint8Array, prefix: Uint8Array): boolean {
  if (bytes.length < prefix.length) return false
  for (let index = 0; index < prefix.length; index += 1) if (bytes[index] !== prefix[index]) return false
  return true
}

/** Decrypts one 4096-byte page and reconstructs its full plaintext, reserved bytes included. */
export function decryptPage(encPage: Uint8Array, pageIndex: number, key: Uint8Array): Uint8Array {
  if (encPage.length !== PAGE_SIZE) throw new Error(`localstorage: page is ${encPage.length} bytes, expected ${PAGE_SIZE}`)
  const pgno = pageIndex + 1
  const reserved = encPage.subarray(RESERVED_OFFSET, RESERVED_OFFSET + RESERVED_SIZE)
  const iv = new Uint8Array(16)
  new DataView(iv.buffer).setUint32(0, pgno, true)
  iv.set(reserved, 4)
  const cipher = createCipheriv('aes-256-cbc', key, iv)
  cipher.setAutoPadding(false)
  const keystream = Buffer.concat([cipher.update(Buffer.alloc(PAGE_SIZE)), cipher.final()])
  const plaintext = new Uint8Array(PAGE_SIZE)
  for (let index = 0; index < RESERVED_OFFSET; index += 1) plaintext[index] = encPage[index]! ^ keystream[index]!
  plaintext.set(reserved, RESERVED_OFFSET)
  if (pageIndex === 0) plaintext.set(encPage.subarray(16, 24), 16)
  return plaintext
}

interface WalParseResult {
  /** 0-based page index to the encrypted (still-ciphered) page bytes from committed frames. */
  pages: Map<number, Uint8Array>
}

/**
 * A WAL frame carries the salt of the WAL generation that wrote it. A
 * checkpoint starts a new generation without erasing the old frames, so a
 * salt mismatch marks the rest of the file as a dead generation, and
 * parsing stops there rather than replaying it. Only frames at or before
 * the last frame that reports a non-zero "database size after commit" are
 * applied: SQLite can leave an uncommitted transaction's frames at the
 * tail, and replaying those would reconstruct a torn state that was never
 * durable.
 */
function parseWal(bytes: Uint8Array): WalParseResult {
  const WAL_HEADER = 32
  const FRAME_HEADER = 24
  const FRAME_SIZE = FRAME_HEADER + PAGE_SIZE
  if (bytes.length <= WAL_HEADER) return { pages: new Map() }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const salt1 = view.getUint32(16, false)
  const salt2 = view.getUint32(20, false)
  const frameCount = Math.floor((bytes.length - WAL_HEADER) / FRAME_SIZE)

  const winningFrame = new Map<number, number>()
  let lastCommitFrame = -1
  for (let frame = 0; frame < frameCount; frame += 1) {
    const offset = WAL_HEADER + frame * FRAME_SIZE
    const pgno = view.getUint32(offset, false)
    const dbSizeAfterCommit = view.getUint32(offset + 4, false)
    const frameSalt1 = view.getUint32(offset + 8, false)
    const frameSalt2 = view.getUint32(offset + 12, false)
    if (frameSalt1 !== salt1 || frameSalt2 !== salt2) break
    if (pgno === 0) continue
    winningFrame.set(pgno - 1, frame)
    if (dbSizeAfterCommit !== 0) lastCommitFrame = frame
  }

  const pages = new Map<number, Uint8Array>()
  for (const [pageIndex, frame] of winningFrame) {
    if (frame > lastCommitFrame) continue
    const start = WAL_HEADER + frame * FRAME_SIZE + FRAME_HEADER
    pages.set(pageIndex, bytes.subarray(start, start + PAGE_SIZE))
  }
  return { pages }
}

/** Overlays WAL pages onto the base pages, extending the array when a frame lands past EOF. */
function mergePages(basePages: Uint8Array[], walPages: Map<number, Uint8Array>): Uint8Array[] {
  const pages = basePages.slice()
  const ceiling = basePages.length + walPages.size
  for (const [index, data] of [...walPages.entries()].sort((a, b) => a[0] - b[0])) {
    if (index < 0 || index >= ceiling) continue
    while (index >= pages.length) pages.push(new Uint8Array(PAGE_SIZE))
    pages[index] = data
  }
  return pages
}

export function candidateDbPaths(home: string): string[] {
  const paths: string[] = []
  const confinement = darwinUserDir()
  if (confinement) paths.push(path.join(confinement, 'com.apple.findmy.findmylocateagent', 'LocalStorage.db'))
  paths.push(path.join(home, 'Library', 'Group Containers', 'group.com.apple.findmy.findmylocateagent', 'Library', 'Application Support', 'LocalStorage.db'))
  return paths
}

/** `$(getconf DARWIN_USER_DIR)` for the calling user. */
function darwinUserDir(): string | null {
  try {
    const proc = Bun.spawnSync(['getconf', 'DARWIN_USER_DIR'])
    const out = new TextDecoder().decode(proc.stdout).trim()
    return out.length > 0 ? out : null
  } catch {
    return null
  }
}

function decryptDb(dbPath: string, key: Uint8Array): Uint8Array | null {
  if (!existsSync(dbPath)) return null
  const dbBytes = new Uint8Array(readFileSync(dbPath))
  const pageCount = Math.floor(dbBytes.length / PAGE_SIZE)
  if (pageCount === 0) return null

  const basePages: Uint8Array[] = []
  for (let index = 0; index < pageCount; index += 1) basePages.push(dbBytes.subarray(index * PAGE_SIZE, (index + 1) * PAGE_SIZE))

  const walPath = `${dbPath}-wal`
  const wal = existsSync(walPath) ? parseWal(new Uint8Array(readFileSync(walPath))) : { pages: new Map<number, Uint8Array>() }
  const encPages = mergePages(basePages, wal.pages)

  const page0 = decryptPage(encPages[0]!, 0, key)
  if (!equalPrefix(page0, SQLITE_MAGIC)) return null

  const out = new Uint8Array(encPages.length * PAGE_SIZE)
  for (let index = 0; index < encPages.length; index += 1) out.set(decryptPage(encPages[index]!, index, key), index * PAGE_SIZE)
  return out
}

/**
 * `findmylocateagent` keeps its store in one of two places, decided when the
 * store is first created. It never migrates afterwards, so every candidate
 * is tried, and each is judged only by whether the key actually decrypts
 * its page 0 to a SQLite header: file size and mtime alone can point at a
 * copy abandoned by an OS upgrade that still holds stale locations with no
 * error.
 */
function resolveDecryptedDb(dbPaths: string[], key: Uint8Array): Uint8Array | null {
  let best: { bytes: Uint8Array; modified: number } | null = null
  for (const candidate of dbPaths) {
    if (!existsSync(candidate)) continue
    const decrypted = decryptDb(candidate, key)
    if (!decrypted) continue
    const modified = statSync(candidate).mtimeMs
    if (!best || modified > best.modified) best = { bytes: decrypted, modified }
  }
  return best?.bytes ?? null
}

export interface FriendRow {
  id: string
  name?: string
  addresses: string[]
  latitude: number
  longitude: number
  accuracy?: number
  timestamp: number
  isSharing: boolean
}

/** CFAbsoluteTime (seconds since 2001-01-01), as stored in `secureLocations.value`, to unix ms. */
const APPLE_EPOCH_OFFSET_MS = 978_307_200_000

function locationTimestampMs(value: PlistValue | undefined): number | undefined {
  if (value instanceof Date) return value.getTime()
  if (typeof value === 'number' && Number.isFinite(value)) return value * 1000 + APPLE_EPOCH_OFFSET_MS
  return undefined
}

/**
 * Decrypts `LocalStorage.db` (and its `-wal`) with the given key, writes the
 * plaintext SQLite file to `sqlitePath`, queries it read-only, then closes
 * it. `sqlitePath`'s directory is created if needed; the file is left in
 * place for inspection but is overwritten on every call. `dbPaths` is
 * usually `candidateDbPaths(homedir())`, taken as a parameter so callers
 * (and tests) can point it at a fixture instead.
 */
export async function readFriendLocations(dbPaths: string[], key: Uint8Array, sqlitePath: string): Promise<FriendRow[]> {
  const decrypted = resolveDecryptedDb(dbPaths, key)
  if (!decrypted) return []
  // The store runs in WAL mode; header bytes 18 and 19 say so, and SQLite then
  // wants a -shm file it cannot create on a read-only open. The WAL frames are
  // already merged above, so present the copy as a plain rollback database.
  decrypted[18] = 1
  decrypted[19] = 1

  await mkdir(path.dirname(sqlitePath), { recursive: true })
  await writeFile(sqlitePath, decrypted, { mode: 0o600 })
  await chmod(sqlitePath, 0o600)

  const db = new Database(sqlitePath, { readonly: true })
  try {
    const rows = db
      .query(
        `SELECT f.handlePrettyName, f.handleContactIdentifier, f.handleIdentifier, f.handleServerIdentifier, f.types, sl.value
         FROM friends f
         JOIN secureLocations sl ON f.handleServerIdentifier = sl.serverUserID`,
      )
      .all() as Array<{
      handlePrettyName: string | null
      handleContactIdentifier: string | null
      handleIdentifier: string | null
      handleServerIdentifier: string | null
      types: string | null
      value: Uint8Array | null
    }>

    const friends: FriendRow[] = []
    for (const row of rows) {
      if (!row.handleServerIdentifier || !row.value) continue
      let location: Record<string, PlistValue> | null = null
      try {
        location = asRecord(parseBinaryPlist(row.value))
      } catch {
        continue
      }
      if (!location) continue
      const latitude = asNumber(location.latitude)
      const longitude = asNumber(location.longitude)
      if (latitude === undefined || longitude === undefined) continue
      friends.push({
        id: row.handleServerIdentifier,
        name: row.handlePrettyName ?? row.handleContactIdentifier ?? undefined,
        addresses: row.handleIdentifier ? [row.handleIdentifier] : [],
        latitude,
        longitude,
        accuracy: asNumber(location.horizontalAccuracy),
        // A row here is always an active share: findmylocateagent removes the
        // secureLocations entry when a friend stops sharing rather than
        // leaving a stale one behind.
        isSharing: true,
        timestamp: locationTimestampMs(location.timestamp) ?? Date.now(),
      })
    }
    return friends
  } finally {
    db.close()
  }
}
