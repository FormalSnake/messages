import { describe, expect, test } from 'bun:test'
import { resolvePinned } from './pinning'

describe('resolvePinned', () => {
  test('passes a 1:1 identifier (phone number or email) through unchanged', () => {
    expect(resolvePinned(['+15551234567', 'friend@example.com'], {})).toEqual(['+15551234567', 'friend@example.com'])
  })

  test('replaces a group key with its resolved `o` chat identifier', () => {
    const groups = { 'C23859EB-55EF-4BE1-890B-D0CE00D19E2D': { o: 'any;+;chat57871837978625300', h: 'deadbeef' } }
    expect(resolvePinned(['C23859EB-55EF-4BE1-890B-D0CE00D19E2D'], groups)).toEqual(['any;+;chat57871837978625300'])
  })

  test('falls back to the raw key when it has no entry in the group table', () => {
    expect(resolvePinned(['unknown-group-key'], {})).toEqual(['unknown-group-key'])
  })

  test('mixes 1:1 identifiers and group keys in one pinned list, preserving order', () => {
    const groups = { 'group-key': { o: 'any;+;chatABC' } }
    expect(resolvePinned(['+15551234567', 'group-key', 'friend@example.com'], groups)).toEqual(['+15551234567', 'any;+;chatABC', 'friend@example.com'])
  })
})
