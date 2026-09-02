import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, test } from 'bun:test'
import { asNumber, asRecord, asString, isBinaryPlist, parseBinaryPlist, parsePayload } from './plist'

// Fixtures are real macOS binary plists, `plutil -convert binary1` over hand-written
// JSON/XML source kept alongside them for reference.
const FIXTURE_DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../desktop/tmp')

function fixture(name: string): Uint8Array {
  return new Uint8Array(readFileSync(path.join(FIXTURE_DIR, name)))
}

describe('parseBinaryPlist on a real plutil-generated bplist', () => {
  test('reads strings, unicode, integers, reals, bools, nesting and arrays', () => {
    const bytes = fixture('sample.bplist')
    expect(isBinaryPlist(bytes)).toBe(true)
    const value = asRecord(parseBinaryPlist(bytes))
    expect(value).not.toBeNull()
    expect(asString(value!.string)).toBe('hello world')
    expect(asString(value!.unicode)).toBe('café Find My 🎯')
    expect(asNumber(value!.int)).toBe(42)
    expect(asNumber(value!.negInt)).toBe(-7)
    expect(asNumber(value!.bigInt)).toBe(4294967296)
    expect(asNumber(value!.real)).toBeCloseTo(3.14159, 5)
    expect(value!.bool_t).toBe(true)
    expect(value!.bool_f).toBe(false)
    expect(value!.array).toEqual(['a', 'b', 'c'])
    expect(value!.emptyArray).toEqual([])
    expect(asRecord(value!.emptyDict)).toEqual({})
    const nested = asRecord(value!.nested)
    expect(asNumber(nested!.a)).toBe(1)
    expect(nested!.b).toEqual([1, 2, 3])
    expect(asString(nested!.c)).toBe('x')
  })

  test('reads dates, data, a long (20-element) array and a signed 8-byte integer', () => {
    const bytes = fixture('sample2.plist')
    const value = asRecord(parseBinaryPlist(bytes))
    expect(value).not.toBeNull()
    expect(value!.date).toBeInstanceOf(Date)
    expect((value!.date as Date).toISOString()).toBe('2025-06-15T12:30:00.000Z')
    expect(value!.data).toBeInstanceOf(Uint8Array)
    expect(new TextDecoder().decode(value!.data as Uint8Array)).toBe('Hello, World!')
    expect(value!.longArray).toEqual(Array.from({ length: 20 }, (_, index) => index))
    expect(asNumber(value!.bigNegative)).toBe(-9223372036854775807)
  })
})

describe('parsePayload', () => {
  test('accepts a binary plist', () => {
    const value = asRecord(parsePayload(fixture('sample.bplist')))
    expect(asString(value!.string)).toBe('hello world')
  })

  test('accepts JSON', () => {
    const value = asRecord(parsePayload(new TextEncoder().encode('{"latitude": 37.5, "longitude": -122.1}')))
    expect(asNumber(value!.latitude)).toBe(37.5)
  })

  test('rejects anything else', () => {
    expect(() => parsePayload(new TextEncoder().encode('not a plist'))).toThrow()
  })
})

describe('asString', () => {
  test('treats the literal "$null" as absent, matching keyed-archiver output', () => {
    expect(asString('$null')).toBeUndefined()
    expect(asString('')).toBeUndefined()
    expect(asString('ok')).toBe('ok')
  })
})
