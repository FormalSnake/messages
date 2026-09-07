import { describe, expect, it } from 'vitest'
import type { Chat, Message } from '@messages/core'
import { buildRows } from './thread'

const chat: Chat = { guid: 'c', identifier: 'c', service: 'iMessage', isGroup: false, participants: [{ address: '+1', service: 'iMessage' }], pinned: false, muted: false, archived: false, unread: false, lastActivity: 0 }

let seq = 0
function message(text: string, date: number, options: Partial<Message> = {}): Message {
  seq += 1
  return { guid: `m${seq}`, chatGuid: 'c', text, fromMe: false, date, service: 'iMessage', attachments: [], tapbacks: [], isAudio: false, ...options }
}

function messageRows(messages: Message[], silencedBy?: string) {
  return buildRows(messages, chat, false, false, true, silencedBy).flatMap((row) => (row.kind === 'message' ? [row] : []))
}

describe('buildRows', () => {
  it('heads a run of replies to the same message with one quote and groups the bubbles', () => {
    const original = message('wat zie je in hem', 0, { fromMe: true })
    const later = message('en?', 500, { fromMe: true })
    const replies = [1000, 2000, 3000].map((date) => message('reply', date, { replyTo: original.guid }))
    const rows = messageRows([original, later, ...replies])
    expect(rows.map((row) => row.showQuote)).toEqual([false, false, true, false, false])
    expect(rows.slice(2).map((row) => row.position)).toEqual(['first', 'middle', 'last'])
  })

  it('starts a new run where a quote appears', () => {
    const old = message('old', 0)
    const filler = message('filler', 100_000)
    const plain = message('plain', 200_000)
    const reply = message('about the old one', 201_000, { replyTo: old.guid })
    const rows = messageRows([old, filler, plain, reply])
    expect(rows.map((row) => row.showQuote)).toEqual([false, false, false, true])
    expect(rows.slice(2).map((row) => row.position)).toEqual(['single', 'single'])
  })
})

describe('a Focus on the other end', () => {
  it('names it under the last message I sent and offers to break through', () => {
    const sent = message('you up', 0, { fromMe: true, dateDelivered: 1 })
    const [row] = messageRows([sent], 'Ben Okafor')
    expect(row?.receipt).toBe('Ben Okafor has notifications silenced')
    expect(row?.notify).toBe(true)
  })

  it('prefers what the message itself came back with', () => {
    const sent = message('you up', 0, { fromMe: true, dateDelivered: 1, deliveredQuietly: true })
    const [row] = messageRows([sent], 'Ben Okafor')
    expect(row?.receipt).toBe('Delivered Quietly')
    expect(row?.notify).toBe(true)
  })

  it('stops offering once it has been broken through, or once they have read it', () => {
    const notified = messageRows([message('you up', 0, { fromMe: true, dateDelivered: 1, deliveredQuietly: true, notified: true })], 'Ben Okafor')
    expect(notified[0]?.receipt).toBe('Notified')
    expect(notified[0]?.notify).toBe(false)
    const read = messageRows([message('you up', 0, { fromMe: true, dateDelivered: 1, dateRead: 2 })], 'Ben Okafor')
    expect(read[0]?.receipt).toMatch(/^Read /)
    expect(read[0]?.notify).toBe(false)
  })
})
