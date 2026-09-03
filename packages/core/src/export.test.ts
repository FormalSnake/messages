import { describe, expect, test } from 'vitest'
import { exportFilePath, formatConversationMarkdown } from './export'
import type { Chat, Message } from './model'

const alex = { address: '+14155550134', service: 'iMessage' as const, name: 'Alex Rivera' }
const ben = { address: '+14155550170', service: 'iMessage' as const, name: 'Ben Okafor' }

function chat(overrides: Partial<Chat> = {}): Chat {
  return {
    guid: 'c1',
    identifier: '+14155550134',
    service: 'iMessage',
    isGroup: false,
    participants: [alex],
    pinned: false,
    muted: false,
    archived: false,
    unread: false,
    lastActivity: 0,
    ...overrides,
  }
}

const day1 = new Date('2026-01-05T09:15:00').getTime()
const day2 = new Date('2026-01-06T18:05:00').getTime()

function message(overrides: Partial<Message> = {}): Message {
  return { guid: 'm1', chatGuid: 'c1', text: 'hi', fromMe: false, sender: alex, date: day1, service: 'iMessage', attachments: [], tapbacks: [], isAudio: false, ...overrides }
}

describe('formatConversationMarkdown', () => {
  test('heads the file with the participants', () => {
    const markdown = formatConversationMarkdown(chat(), [alex], [])
    expect(markdown).toMatch(/^# Alex Rivera\n/)
  })

  test('lists every participant for a group', () => {
    const markdown = formatConversationMarkdown(chat({ isGroup: true }), [alex, ben], [])
    expect(markdown).toMatch(/^# Alex Rivera, Ben Okafor\n/)
  })

  test('formats a line as **Sender** HH:MM  text', () => {
    const markdown = formatConversationMarkdown(chat(), [alex], [message({ text: 'coffee at 4?' })])
    expect(markdown).toContain('**Alex Rivera** 09:15  coffee at 4?')
  })

  test('labels the user\'s own messages "You"', () => {
    const markdown = formatConversationMarkdown(chat(), [alex], [message({ fromMe: true, sender: undefined, text: 'sure' })])
    expect(markdown).toContain('**You** 09:15  sure')
  })

  test('inserts a day separator when the date changes, once per day', () => {
    const markdown = formatConversationMarkdown(chat(), [alex], [message({ date: day1 }), message({ guid: 'm2', date: day1 + 60_000 }), message({ guid: 'm3', date: day2 })])
    expect(markdown.match(/^## /gm)).toHaveLength(2)
  })

  test('renders attachments as a trailing [name]', () => {
    const markdown = formatConversationMarkdown(chat(), [alex], [message({ text: '', attachments: [{ guid: 'a1', name: 'IMG_1.jpg', mime: 'image/jpeg', bytes: 100, isSticker: false, hidden: false }] })])
    expect(markdown).toContain('**Alex Rivera** 09:15  [IMG_1.jpg]')
  })

  test('drops hidden attachments, such as a link preview image', () => {
    const markdown = formatConversationMarkdown(chat(), [alex], [message({ text: 'a link', attachments: [{ guid: 'a1', name: 'preview.jpg', mime: 'image/jpeg', bytes: 100, isSticker: false, hidden: true }] })])
    expect(markdown).not.toContain('preview.jpg')
  })

  test('renders tapbacks as a trailing count list', () => {
    const markdown = formatConversationMarkdown(
      chat(),
      [alex],
      [message({ text: 'shipped', tapbacks: [{ guid: 't1', kind: 'love', fromMe: true }, { guid: 't2', kind: 'love', fromMe: false, sender: ben }, { guid: 't3', kind: 'like', fromMe: false, sender: ben }] })],
    )
    expect(markdown).toContain('**Alex Rivera** 09:15  shipped (❤️ 2, 👍 1)')
  })

  test('describes a retracted message instead of showing its text', () => {
    const markdown = formatConversationMarkdown(chat(), [alex], [message({ text: 'oops', dateRetracted: day1 })])
    expect(markdown).toContain('**Alex Rivera** 09:15  unsent a message')
  })

  test('describes a group event instead of its empty text', () => {
    const markdown = formatConversationMarkdown(chat({ isGroup: true }), [alex, ben], [message({ text: '', groupEvent: { kind: 'rename', title: 'Family' } })])
    expect(markdown).toContain('**Alex Rivera** 09:15  named the conversation "Family"')
  })
})

describe('exportFilePath', () => {
  test('lands in Downloads, named after the title and the export date', () => {
    const filePath = exportFilePath(chat({ displayName: 'Family Trip' }), new Date('2026-03-04T12:00:00'))
    expect(filePath.endsWith('/Downloads/Family Trip 2026-03-04.md')).toBe(true)
  })

  test('replaces characters a filesystem would reject', () => {
    const filePath = exportFilePath(chat({ displayName: 'Q1/Q2 planning' }), new Date('2026-03-04T12:00:00'))
    expect(filePath.endsWith('/Downloads/Q1-Q2 planning 2026-03-04.md')).toBe(true)
  })
})
