import { describe, expect, test } from 'vitest'
import { parseSearchQuery, resolveSearchQuery, type SearchContext } from './search'
import type { Chat, Contact } from './model'

function contact(name: string, addresses: string[]): Contact {
  return { id: name, name, addresses }
}

function chat(guid: string, participants: SearchContext['chats'][number]['participants']): Chat {
  return { guid, identifier: guid, service: 'iMessage', isGroup: false, participants, pinned: false, muted: false, archived: false, unread: false, lastActivity: 0 }
}

describe('parseSearchQuery', () => {
  test('plain text with no operators', () => {
    expect(parseSearchQuery('coffee tomorrow')).toEqual({ text: 'coffee tomorrow', fromMe: false, senders: [], links: false, chatNames: [] })
  })

  test('from:me sets fromMe and leaves no sender', () => {
    const parsed = parseSearchQuery('from:me lunch')
    expect(parsed.fromMe).toBe(true)
    expect(parsed.senders).toEqual([])
    expect(parsed.text).toBe('lunch')
  })

  test('from:name collects the raw token, unresolved', () => {
    const parsed = parseSearchQuery('from:Alex bike')
    expect(parsed.senders).toEqual(['Alex'])
    expect(parsed.text).toBe('bike')
  })

  test('a quoted from: value keeps its spaces as one token', () => {
    const parsed = parseSearchQuery('from:"Priya Natarajan" deck')
    expect(parsed.senders).toEqual(['Priya Natarajan'])
    expect(parsed.text).toBe('deck')
  })

  test('has:photo, has:video, has:file, has:link', () => {
    expect(parseSearchQuery('has:photo').attachments).toBe('image')
    expect(parseSearchQuery('has:video').attachments).toBe('video')
    expect(parseSearchQuery('has:file').attachments).toBe('file')
    expect(parseSearchQuery('has:link').links).toBe(true)
  })

  test('an unknown has: value falls back to plain text', () => {
    const parsed = parseSearchQuery('has:sticker')
    expect(parsed.attachments).toBeUndefined()
    expect(parsed.text).toBe('has:sticker')
  })

  test('before: and after: parse a local date to midnight', () => {
    const parsed = parseSearchQuery('after:2024-01-15 before:2024-02-01')
    expect(parsed.after).toBe(new Date(2024, 0, 15).getTime())
    expect(parsed.before).toBe(new Date(2024, 1, 1).getTime())
  })

  test('a malformed date falls back to plain text', () => {
    const parsed = parseSearchQuery('before:not-a-date')
    expect(parsed.before).toBeUndefined()
    expect(parsed.text).toBe('before:not-a-date')
  })

  test('in:chat name collects the raw token', () => {
    const parsed = parseSearchQuery('in:Family lunch')
    expect(parsed.chatNames).toEqual(['Family'])
    expect(parsed.text).toBe('lunch')
  })

  test('every operator combines with free text', () => {
    const parsed = parseSearchQuery('from:me has:photo after:2024-01-01 in:Family the hike photos')
    expect(parsed).toEqual({
      text: 'the hike photos',
      fromMe: true,
      senders: [],
      attachments: 'image',
      links: false,
      after: new Date(2024, 0, 1).getTime(),
      chatNames: ['Family'],
    })
  })
})

describe('resolveSearchQuery', () => {
  const context: SearchContext = {
    contacts: [contact('Alex Rivera', ['+14155550134', 'alex@example.com']), contact('Priya Natarajan', ['priya@example.com'])],
    chats: [
      chat('iMessage;-;+14155550188', [{ address: '+14155550188', service: 'iMessage', name: 'Jordan Lee' }]),
      chat('iMessage;+;chat1', [{ address: '+14155550101', service: 'iMessage', name: 'Mom' }]),
    ],
  }

  test('a from: name resolves to every address of the matching contact', () => {
    const { filters } = resolveSearchQuery(parseSearchQuery('from:Alex'), context)
    expect(filters.senders).toEqual(['+14155550134', 'alex@example.com'])
  })

  test('a from: name with no contact match falls back to a chat participant', () => {
    const { filters } = resolveSearchQuery(parseSearchQuery('from:Jordan'), context)
    expect(filters.senders).toEqual(['+14155550188'])
  })

  test('a from: value with no match anywhere is passed through as a literal address', () => {
    const { filters } = resolveSearchQuery(parseSearchQuery('from:+14155559999'), context)
    expect(filters.senders).toEqual(['+14155559999'])
  })

  test('from:me sets fromMe with no senders filter', () => {
    const { filters } = resolveSearchQuery(parseSearchQuery('from:me'), context)
    expect(filters.fromMe).toBe(true)
    expect(filters.senders).toBeUndefined()
  })

  test('in:chat name resolves to matching chat guids', () => {
    const { filters, text } = resolveSearchQuery(parseSearchQuery('in:Family sunday'), context)
    expect(filters.chatGuids).toEqual([])
    expect(text).toBe('sunday')
  })

  test('in:mom resolves to the chat whose participant is named that', () => {
    const { filters } = resolveSearchQuery(parseSearchQuery('in:Mom'), context)
    expect(filters.chatGuids).toEqual(['iMessage;+;chat1'])
  })

  test('before: shifts back one millisecond so the named day is excluded', () => {
    const midnight = new Date(2024, 1, 1).getTime()
    const { filters } = resolveSearchQuery(parseSearchQuery('before:2024-02-01'), context)
    expect(filters.before).toBe(midnight - 1)
  })

  test('no operators leaves an empty filters object', () => {
    const { filters, text } = resolveSearchQuery(parseSearchQuery('just words'), context)
    expect(filters).toEqual({})
    expect(text).toBe('just words')
  })
})
