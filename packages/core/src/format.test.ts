import { describe, expect, test } from 'vitest'
import { parseScheduleTime } from './format'

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
