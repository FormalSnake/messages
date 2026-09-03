import { describe, expect, test } from 'vitest'
import { buildSearchWhere } from './client'

describe('buildSearchWhere', () => {
  test('no filters produces no clauses', () => {
    expect(buildSearchWhere({})).toEqual([])
  })

  test('fromMe', () => {
    expect(buildSearchWhere({ fromMe: true })).toEqual([{ statement: 'message.is_from_me = :fromMe', args: { fromMe: 1 } }])
  })

  test('senders join on the handle table', () => {
    expect(buildSearchWhere({ senders: ['+14155550134', 'alex@example.com'] })).toEqual([
      { statement: 'handle.id IN (:...senders)', args: { senders: ['+14155550134', 'alex@example.com'] } },
    ])
  })

  test('an empty senders array adds no clause', () => {
    expect(buildSearchWhere({ senders: [] })).toEqual([])
  })

  test('attachments: image, video and file', () => {
    expect(buildSearchWhere({ attachments: 'image' })).toEqual([{ statement: "attachment.mime_type LIKE 'image/%'" }])
    expect(buildSearchWhere({ attachments: 'video' })).toEqual([{ statement: "attachment.mime_type LIKE 'video/%'" }])
    expect(buildSearchWhere({ attachments: 'file' })[0]?.statement).toContain("NOT LIKE 'image/%'")
  })

  test('links matches a link preview or a bare url in the text', () => {
    expect(buildSearchWhere({ links: true })).toEqual([
      {
        statement: '(message.balloon_bundle_id = :linkBundle OR message.text LIKE :linkText)',
        args: { linkBundle: 'com.apple.messages.URLBalloonProvider', linkText: '%http%' },
      },
    ])
  })

  test('chatGuids filters to the given chats', () => {
    expect(buildSearchWhere({ chatGuids: ['a', 'b'] })).toEqual([{ statement: 'chat.guid IN (:...chatGuids)', args: { chatGuids: ['a', 'b'] } }])
  })

  test('an empty chatGuids array (in: matched no chat) returns nothing rather than everything', () => {
    expect(buildSearchWhere({ chatGuids: [] })).toEqual([{ statement: '1 = 0' }])
  })

  test('filters combine into one clause per filter', () => {
    const where = buildSearchWhere({ fromMe: true, attachments: 'video', chatGuids: ['a'] })
    expect(where).toHaveLength(3)
  })
})
