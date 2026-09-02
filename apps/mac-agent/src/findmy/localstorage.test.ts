import { createCipheriv } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { Database } from 'bun:sqlite'
import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { PAGE_SIZE, candidateDbPaths, decryptPage, readFriendLocations } from './localstorage'

const KEY = new Uint8Array(32)
for (let index = 0; index < 32; index += 1) KEY[index] = (index * 7 + 3) % 256

/**
 * The inverse of `decryptPage`, used only here to build a realistic encrypted
 * fixture from a real plaintext SQLite page: XOR the content with the same
 * AES-CBC keystream, then (page 0 only) leave the header bytes at 16..24
 * unencrypted, exactly as `decryptPage`'s fix-up assumes.
 */
function encryptPage(plainPage: Uint8Array, pageIndex: number, key: Uint8Array): Uint8Array {
  const pgno = pageIndex + 1
  const reserved = plainPage.subarray(4084, 4096)
  const iv = new Uint8Array(16)
  new DataView(iv.buffer).setUint32(0, pgno, true)
  iv.set(reserved, 4)
  const cipher = createCipheriv('aes-256-cbc', key, iv)
  cipher.setAutoPadding(false)
  const keystream = Buffer.concat([cipher.update(Buffer.alloc(PAGE_SIZE)), cipher.final()])
  const encPage = new Uint8Array(PAGE_SIZE)
  for (let index = 0; index < 4084; index += 1) encPage[index] = plainPage[index]! ^ keystream[index]!
  encPage.set(reserved, 4084)
  if (pageIndex === 0) encPage.set(plainPage.subarray(16, 24), 16)
  return encPage
}

function encryptWholeFile(bytes: Uint8Array, key: Uint8Array): Uint8Array {
  const pageCount = bytes.length / PAGE_SIZE
  if (!Number.isInteger(pageCount)) throw new Error(`test fixture: ${bytes.length} is not a multiple of ${PAGE_SIZE}`)
  const out = new Uint8Array(bytes.length)
  for (let index = 0; index < pageCount; index += 1) out.set(encryptPage(bytes.subarray(index * PAGE_SIZE, (index + 1) * PAGE_SIZE), index, key), index * PAGE_SIZE)
  return out
}

const FRIEND1_LOCATION = path.join(__dirname, '../../../desktop/tmp/friend1-location.plist')
const FRIEND2_LOCATION = path.join(__dirname, '../../../desktop/tmp/friend2-location.plist')

let workDir: string

beforeEach(() => {
  workDir = mkdtempSync(path.join(tmpdir(), 'findmy-localstorage-'))
})

afterEach(() => {
  rmSync(workDir, { recursive: true, force: true })
})

/** Builds a plaintext friends/secureLocations SQLite db at `dbPath`, 4096-byte pages, no WAL residue. */
function buildPlainDb(dbPath: string): void {
  const db = new Database(dbPath, { create: true })
  db.exec('PRAGMA page_size = 4096')
  db.exec('PRAGMA journal_mode = DELETE')
  db.exec('CREATE TABLE friends (handleServerIdentifier TEXT PRIMARY KEY, handlePrettyName TEXT, handleContactIdentifier TEXT, handleIdentifier TEXT, types TEXT)')
  db.exec('CREATE TABLE secureLocations (serverUserID TEXT, value BLOB)')
  db.run('INSERT INTO friends VALUES (?, ?, ?, ?, ?)', ['server-1', 'Ana', 'ana@example.com', '+14155551234', 'friend'])
  db.run('INSERT INTO friends VALUES (?, ?, ?, ?, ?)', ['server-2', null, null, '+442071234567', 'friend'])
  db.run('INSERT INTO secureLocations VALUES (?, ?)', ['server-1', new Uint8Array(readFileSync(FRIEND1_LOCATION))])
  db.run('INSERT INTO secureLocations VALUES (?, ?)', ['server-2', new Uint8Array(readFileSync(FRIEND2_LOCATION))])
  db.close()
}

describe('decryptPage', () => {
  test('round-trips a real SQLite page 0 through encryptPage/decryptPage, header fix-up included', () => {
    const dbPath = path.join(workDir, 'plain.db')
    buildPlainDb(dbPath)
    const plainBytes = new Uint8Array(readFileSync(dbPath))
    const page0 = plainBytes.subarray(0, PAGE_SIZE)
    expect(new TextDecoder().decode(page0.subarray(0, 16))).toBe('SQLite format 3\0')

    const encrypted = encryptPage(page0, 0, KEY)
    const decrypted = decryptPage(encrypted, 0, KEY)
    expect(decrypted).toEqual(page0)
  })

  test('a wrong key does not reproduce the SQLite magic', () => {
    const dbPath = path.join(workDir, 'plain.db')
    buildPlainDb(dbPath)
    const page0 = new Uint8Array(readFileSync(dbPath)).subarray(0, PAGE_SIZE)
    const encrypted = encryptPage(page0, 0, KEY)
    const wrongKey = new Uint8Array(32).fill(9)
    const decrypted = decryptPage(encrypted, 0, wrongKey)
    expect(new TextDecoder().decode(decrypted.subarray(0, 16))).not.toBe('SQLite format 3\0')
  })
})

describe('readFriendLocations', () => {
  test('decrypts an encrypted LocalStorage.db and queries the friends/secureLocations join', async () => {
    const plainPath = path.join(workDir, 'plain.db')
    buildPlainDb(plainPath)
    const encrypted = encryptWholeFile(new Uint8Array(readFileSync(plainPath)), KEY)

    const home = path.join(workDir, 'home')
    const dbPaths = candidateDbPaths(home)
    // The group-container path is the one under our control; the confinement
    // path comes from the real machine's DARWIN_USER_DIR and simply won't
    // exist for this fixture.
    const groupContainerPath = dbPaths[dbPaths.length - 1]!
    mkdirSync(path.dirname(groupContainerPath), { recursive: true })
    writeFileSync(groupContainerPath, encrypted, { mode: 0o600 })

    const sqliteOut = path.join(workDir, 'decrypted.sqlite')
    const friends = await readFriendLocations(dbPaths, KEY, sqliteOut)

    expect(existsSync(sqliteOut)).toBe(true)
    expect(friends).toHaveLength(2)

    const ana = friends.find((friend) => friend.id === 'server-1')!
    expect(ana.name).toBe('Ana')
    expect(ana.addresses).toEqual(['+14155551234'])
    expect(ana.latitude).toBeCloseTo(37.334722, 5)
    expect(ana.longitude).toBeCloseTo(-122.008889, 5)
    expect(ana.accuracy).toBeCloseTo(8.5, 5)
    // 773700000 CFAbsoluteTime seconds -> 2025-07-08T20:40:00.000Z
    expect(new Date(ana.timestamp).toISOString()).toBe('2025-07-08T20:40:00.000Z')
    expect(ana.isSharing).toBe(true)

    const other = friends.find((friend) => friend.id === 'server-2')!
    // No pretty name or contact identifier on this row: falls back to undefined, not the raw handle.
    expect(other.name).toBeUndefined()
    expect(other.addresses).toEqual(['+442071234567'])
    expect(other.latitude).toBeCloseTo(51.5072, 4)
  })

  test('returns nothing when no candidate database exists', async () => {
    const home = path.join(workDir, 'nobody-home')
    const friends = await readFriendLocations(candidateDbPaths(home), KEY, path.join(workDir, 'out.sqlite'))
    expect(friends).toEqual([])
  })

  test('returns nothing when the key does not decrypt any candidate to a valid SQLite header', async () => {
    const plainPath = path.join(workDir, 'plain.db')
    buildPlainDb(plainPath)
    const encrypted = encryptWholeFile(new Uint8Array(readFileSync(plainPath)), KEY)
    const home = path.join(workDir, 'home2')
    const dbPaths = candidateDbPaths(home)
    const groupContainerPath = dbPaths[dbPaths.length - 1]!
    mkdirSync(path.dirname(groupContainerPath), { recursive: true })
    writeFileSync(groupContainerPath, encrypted, { mode: 0o600 })

    const wrongKey = new Uint8Array(32).fill(9)
    const friends = await readFriendLocations(dbPaths, wrongKey, path.join(workDir, 'out2.sqlite'))
    expect(friends).toEqual([])
  })

  test('overlays a WAL frame written by a real SQLite connection', async () => {
    const plainPath = path.join(workDir, 'plain.db')
    buildPlainDb(plainPath)

    // The base snapshot is captured before any WAL activity, so it stays the
    // pre-update state; the update itself lives only in the WAL frame.
    const encryptedMain = encryptWholeFile(new Uint8Array(readFileSync(plainPath)), KEY)

    // Reopen in WAL mode, make a committed change, and stop auto-checkpoint.
    // bun:sqlite checkpoints (and truncates) the WAL on `close()` even with
    // another connection still open, so the WAL bytes have to be read back
    // before closing this connection, not after.
    const db = new Database(plainPath)
    db.exec('PRAGMA journal_mode = WAL')
    db.exec('PRAGMA wal_autocheckpoint = 0')
    db.run('UPDATE secureLocations SET value = ? WHERE serverUserID = ?', [new Uint8Array(readFileSync(FRIEND2_LOCATION)), 'server-1'])
    expect(existsSync(`${plainPath}-wal`)).toBe(true)

    // Only the 4096-byte page payload inside each frame is enciphered; the
    // 24-byte frame header (pgno, commit size, salts, checksum) stays as
    // SQLite wrote it, matching the facts' WAL layout.
    const walBytes = new Uint8Array(readFileSync(`${plainPath}-wal`))
    db.close()
    const walView = new DataView(walBytes.buffer, walBytes.byteOffset, walBytes.byteLength)
    const WAL_HEADER = 32
    const FRAME_HEADER = 24
    const FRAME_SIZE = FRAME_HEADER + PAGE_SIZE
    const frameCount = Math.floor((walBytes.length - WAL_HEADER) / FRAME_SIZE)
    const encryptedWal = walBytes.slice()
    for (let frame = 0; frame < frameCount; frame += 1) {
      const frameOffset = WAL_HEADER + frame * FRAME_SIZE
      const pgno = walView.getUint32(frameOffset, false)
      const pageStart = frameOffset + FRAME_HEADER
      const plainPage = walBytes.subarray(pageStart, pageStart + PAGE_SIZE)
      encryptedWal.set(encryptPage(plainPage, pgno - 1, KEY), pageStart)
    }

    const home = path.join(workDir, 'home-wal')
    const dbPaths = candidateDbPaths(home)
    const groupContainerPath = dbPaths[dbPaths.length - 1]!
    mkdirSync(path.dirname(groupContainerPath), { recursive: true })
    writeFileSync(groupContainerPath, encryptedMain, { mode: 0o600 })
    writeFileSync(`${groupContainerPath}-wal`, encryptedWal, { mode: 0o600 })

    const friends = await readFriendLocations(dbPaths, KEY, path.join(workDir, 'out-wal.sqlite'))
    const ana = friends.find((friend) => friend.id === 'server-1')!
    // Ana's location was overwritten with friend2's coordinates via the WAL.
    expect(ana.latitude).toBeCloseTo(51.5072, 4)
    expect(ana.longitude).toBeCloseTo(-0.1276, 4)
  })
})
