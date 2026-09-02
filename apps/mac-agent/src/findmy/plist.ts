/**
 * Binary property list (bplist00) reader. Every Find My payload lands as one:
 * the key files, the sealed `.data` caches, and the `secureLocations.value`
 * blobs inside LocalStorage.db. Bun has no plist support and macOS `plutil`
 * would mean a subprocess per blob.
 *
 * Format reference: CoreFoundation's CFBinaryPList.c.
 */

export type PlistValue = null | boolean | number | string | Date | Uint8Array | PlistValue[] | { [key: string]: PlistValue }

const HEADER = 'bplist00'
/** CFAbsoluteTime is seconds since 2001-01-01; Unix time is seconds since 1970-01-01. */
export const APPLE_EPOCH_OFFSET_MS = 978_307_200_000

class Reader {
  private view: DataView

  constructor(
    private bytes: Uint8Array,
    private offsetSize: number,
    private refSize: number,
    private offsets: number[],
  ) {
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  }

  static parse(bytes: Uint8Array): PlistValue {
    if (bytes.length < 40) throw new Error('bplist: shorter than a trailer')
    const magic = new TextDecoder().decode(bytes.subarray(0, 8))
    if (magic !== HEADER) throw new Error(`bplist: bad magic ${JSON.stringify(magic)}`)
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
    const trailer = bytes.length - 32
    const offsetSize = view.getUint8(trailer + 6)
    const refSize = view.getUint8(trailer + 7)
    const count = Number(view.getBigUint64(trailer + 8))
    const top = Number(view.getBigUint64(trailer + 16))
    const tableAt = Number(view.getBigUint64(trailer + 24))
    if (offsetSize < 1 || offsetSize > 8 || refSize < 1 || refSize > 8) throw new Error('bplist: bad trailer sizes')
    const offsets: number[] = []
    for (let index = 0; index < count; index += 1) offsets.push(readInt(view, tableAt + index * offsetSize, offsetSize))
    return new Reader(bytes, offsetSize, refSize, offsets).object(top)
  }

  private ref(at: number): number {
    return readInt(this.view, at, this.refSize)
  }

  private object(index: number): PlistValue {
    const start = this.offsets[index]
    if (start === undefined) throw new Error(`bplist: object ${index} is out of range`)
    const marker = this.view.getUint8(start)
    const kind = marker >> 4
    const low = marker & 0x0f
    switch (kind) {
      case 0x0:
        if (low === 0x00) return null
        if (low === 0x08) return false
        if (low === 0x09) return true
        // 0x0f is the fill byte and 0x0c/0x0e are URL/UUID markers CoreFoundation
        // never writes into these caches.
        return null
      case 0x1:
        return this.integer(start + 1, 1 << low)
      case 0x2:
        return low === 2 ? this.view.getFloat32(start + 1) : this.view.getFloat64(start + 1)
      case 0x3:
        return new Date(this.view.getFloat64(start + 1) * 1000 + APPLE_EPOCH_OFFSET_MS)
      case 0x4: {
        const { length, at } = this.sized(start, low)
        return this.bytes.slice(at, at + length)
      }
      case 0x5: {
        const { length, at } = this.sized(start, low)
        return latin1(this.bytes.subarray(at, at + length))
      }
      case 0x6: {
        const { length, at } = this.sized(start, low)
        let out = ''
        for (let index = 0; index < length; index += 1) out += String.fromCharCode(this.view.getUint16(at + index * 2))
        return out
      }
      case 0x8:
        // A CFKeyedArchiver UID. Nothing here dereferences one; the number is enough.
        return this.integer(start + 1, low + 1)
      case 0xa:
      case 0xc: {
        const { length, at } = this.sized(start, low)
        const out: PlistValue[] = []
        for (let index = 0; index < length; index += 1) out.push(this.object(this.ref(at + index * this.refSize)))
        return out
      }
      case 0xd: {
        const { length, at } = this.sized(start, low)
        const out: { [key: string]: PlistValue } = {}
        for (let index = 0; index < length; index += 1) {
          const key = this.object(this.ref(at + index * this.refSize))
          const value = this.object(this.ref(at + (length + index) * this.refSize))
          out[typeof key === 'string' ? key : String(key)] = value
        }
        return out
      }
      default:
        throw new Error(`bplist: unsupported marker 0x${marker.toString(16)}`)
    }
  }

  /** A low nibble of 0xf means the real count is the integer object that follows. */
  private sized(start: number, low: number): { length: number; at: number } {
    if (low !== 0x0f) return { length: low, at: start + 1 }
    const marker = this.view.getUint8(start + 1)
    if (marker >> 4 !== 0x1) throw new Error('bplist: expected an integer length')
    const width = 1 << (marker & 0x0f)
    return { length: readInt(this.view, start + 2, width), at: start + 2 + width }
  }

  private integer(at: number, width: number): number {
    // Only the 8-byte form is signed; CoreFoundation writes smaller widths unsigned.
    if (width === 8) return Number(this.view.getBigInt64(at))
    if (width === 16) return Number(this.view.getBigInt64(at) * (1n << 64n) + this.view.getBigUint64(at + 8))
    return readInt(this.view, at, width)
  }
}

function readInt(view: DataView, at: number, width: number): number {
  let value = 0
  for (let index = 0; index < width; index += 1) value = value * 256 + view.getUint8(at + index)
  return value
}

function latin1(bytes: Uint8Array): string {
  let out = ''
  for (const byte of bytes) out += String.fromCharCode(byte)
  return out
}

export function isBinaryPlist(bytes: Uint8Array): boolean {
  return bytes.length >= 8 && latin1(bytes.subarray(0, 8)) === HEADER
}

export function parseBinaryPlist(bytes: Uint8Array): PlistValue {
  return Reader.parse(bytes)
}

/**
 * A decrypted cache is a binary plist on every machine measured, but the
 * FindMyCrypto envelope carries no content type, so JSON is accepted too.
 */
export function parsePayload(bytes: Uint8Array): PlistValue {
  if (isBinaryPlist(bytes)) return parseBinaryPlist(bytes)
  const text = new TextDecoder().decode(bytes).trim()
  if (text.startsWith('{') || text.startsWith('[')) return JSON.parse(text) as PlistValue
  throw new Error('findmy: decrypted payload is neither a binary plist nor JSON')
}

export function asRecord(value: PlistValue | undefined): Record<string, PlistValue> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value) && !(value instanceof Date) && !(value instanceof Uint8Array)
    ? (value as Record<string, PlistValue>)
    : null
}

export function asNumber(value: PlistValue | undefined): number | undefined {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  if (value instanceof Date) return value.getTime()
  return undefined
}

export function asString(value: PlistValue | undefined): string | undefined {
  // Apple writes the literal "$null" where a keyed archiver had no value.
  return typeof value === 'string' && value.length > 0 && value !== '$null' ? value : undefined
}
