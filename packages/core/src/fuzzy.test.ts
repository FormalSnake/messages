import { describe, expect, test } from 'vitest'
import { fuzzyScore, searchChats } from './fuzzy'
import type { Chat } from './model'

describe('fuzzyScore', () => {
  test('matches a subsequence regardless of case', () => {
    expect(fuzzyScore('mrn', 'Maureen')).not.toBeNull()
  })

  test('rejects characters out of order', () => {
    expect(fuzzyScore('nam', 'Maureen')).toBeNull()
  })

  test('rejects a character missing entirely', () => {
    expect(fuzzyScore('zz', 'Maureen')).toBeNull()
  })

  test('empty query matches everything with a zero score', () => {
    expect(fuzzyScore('', 'Maureen')).toBe(0)
  })

  test('scores a match at a word start higher than one mid-word', () => {
    const wordStart = fuzzyScore('m', 'Cape May')
    const midWord = fuzzyScore('m', 'Cape Cod Marina')
    expect(wordStart).not.toBeNull()
    expect(midWord).not.toBeNull()
    expect(wordStart! > midWord!).toBe(true)
  })

  test('prefers a shorter title for an equally good match', () => {
    const short = fuzzyScore('ann', 'Ann')
    const long = fuzzyScore('ann', 'Ann Marie Fitzgerald-Robinson')
    expect(short).not.toBeNull()
    expect(long).not.toBeNull()
    expect(short! > long!).toBe(true)
  })

  test('a longer consecutive run scores higher than the same letters scattered', () => {
    const consecutive = fuzzyScore('ann', 'Ann Smith')
    const scattered = fuzzyScore('ann', 'Anna Nolan Nunez')
    expect(consecutive! > scattered!).toBe(true)
  })
})

function chat(overrides: Partial<Chat>): Chat {
  return {
    guid: 'g1',
    identifier: 'g1',
    service: 'iMessage',
    isGroup: false,
    participants: [],
    pinned: false,
    muted: false,
    archived: false,
    unread: false,
    lastActivity: 0,
    ...overrides,
  }
}

describe('searchChats', () => {
  const chats: Chat[] = [
    chat({ guid: '1', identifier: '1', displayName: 'Weekend trip' }),
    chat({ guid: '2', identifier: '2', participants: [{ address: '+15551234567', service: 'iMessage', name: 'Maureen Doyle' }] }),
    chat({ guid: '3', identifier: '3', participants: [{ address: 'sam@example.com', service: 'iMessage' }] }),
  ]

  test('returns every chat, most recently active first, for an empty query', () => {
    expect(searchChats(chats, '').map((c) => c.guid)).toEqual(['1', '2', '3'])
  })

  test('matches a chat by its display title', () => {
    expect(searchChats(chats, 'weekend').map((c) => c.guid)).toEqual(['1'])
  })

  test('matches a chat by a participant name', () => {
    expect(searchChats(chats, 'maur').map((c) => c.guid)).toEqual(['2'])
  })

  test('matches a chat by a participant address', () => {
    expect(searchChats(chats, 'sam@example').map((c) => c.guid)).toEqual(['3'])
  })

  test('drops chats that match nothing', () => {
    expect(searchChats(chats, 'zzzzz')).toEqual([])
  })

  test('caps results at the given limit', () => {
    const many = Array.from({ length: 12 }, (_, i) => chat({ guid: `m${i}`, identifier: `m${i}`, displayName: 'Team standup' }))
    expect(searchChats(many, 'team', 8)).toHaveLength(8)
  })
})
