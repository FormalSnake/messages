import { describe, expect, test } from 'vitest'
import { isEmojiOnly, parseScheduleTime } from './format'

const NOW = new Date(2026, 8, 3, 14, 30, 0, 0).getTime() // Thu Sep 3 2026, 14:30 local

describe('parseScheduleTime', () => {
  test('parses a bare time as today', () => {
    const result = parseScheduleTime('20:00', NOW)
    expect(result).toEqual({ sendAt: new Date(2026, 8, 3, 20, 0, 0, 0).getTime() })
  })

  test('parses "tomorrow HH:MM"', () => {
    const result = parseScheduleTime('tomorrow 09:00', NOW)
    expect(result).toEqual({ sendAt: new Date(2026, 8, 4, 9, 0, 0, 0).getTime() })
  })

  test('parses "tomorrow" case-insensitively with a single-digit hour', () => {
    const result = parseScheduleTime('Tomorrow 9:05', NOW)
    expect(result).toEqual({ sendAt: new Date(2026, 8, 4, 9, 5, 0, 0).getTime() })
  })

  test('parses a full date and time', () => {
    const result = parseScheduleTime('2026-12-25 08:00', NOW)
    expect(result).toEqual({ sendAt: new Date(2026, 11, 25, 8, 0, 0, 0).getTime() })
  })

  test('rejects a bare time already in the past today', () => {
    expect(parseScheduleTime('09:00', NOW)).toEqual({ error: 'That time has already passed' })
  })

  test('rejects a full date and time already in the past', () => {
    expect(parseScheduleTime('2026-09-03 09:00', NOW)).toEqual({ error: 'That time has already passed' })
  })

  test('rejects an unparsable string', () => {
    expect('error' in parseScheduleTime('whenever', NOW)).toBe(true)
  })

  test('rejects an out-of-range hour', () => {
    expect('error' in parseScheduleTime('25:00', NOW)).toBe(true)
  })

  test('rejects an out-of-range minute', () => {
    expect('error' in parseScheduleTime('12:75', NOW)).toBe(true)
  })

  test('rejects a date that does not exist', () => {
    expect('error' in parseScheduleTime('2026-02-30 08:00', NOW)).toBe(true)
  })

  test('rejects empty input', () => {
    expect('error' in parseScheduleTime('   ', NOW)).toBe(true)
  })
})

describe('isEmojiOnly', () => {
  test('accepts a country flag, which is two code points', () => {
    expect(isEmojiOnly('\u{1F1F9}\u{1F1F7}')).toBe(true)
    expect(isEmojiOnly('\u{1F1F9}\u{1F1F7} \u{1F1EA}\u{1F1F8}')).toBe(true)
  })

  test('accepts ZWJ sequences, skin tones, keycaps and tag flags', () => {
    expect(isEmojiOnly('\u{1F3F3}\uFE0F\u200D\u{1F308}')).toBe(true)
    expect(isEmojiOnly('\u{1F44B}\u{1F3FD}')).toBe(true)
    expect(isEmojiOnly('1\uFE0F\u20E3')).toBe(true)
    expect(isEmojiOnly('\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}')).toBe(true)
  })

  test('rejects text, empty strings and more than the allowed count', () => {
    expect(isEmojiOnly('')).toBe(false)
    expect(isEmojiOnly('ok \u{1F44D}')).toBe(false)
    expect(isEmojiOnly('5')).toBe(false)
    expect(isEmojiOnly('\u{1F600}\u{1F600}\u{1F600}\u{1F600}')).toBe(false)
    expect(isEmojiOnly('\u{1F1F9}\u{1F1F7}\u{1F1EA}\u{1F1F8}\u{1F1EB}\u{1F1F7}\u{1F1F3}\u{1F1F1}')).toBe(false)
  })
})
