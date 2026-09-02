import { describe, expect, test } from 'vitest'
import {
  ContactIndex,
  decodeKeyedArchive,
  downloadPlan,
  toChat,
  toContact,
  toHandle,
  toMessage,
  toParts,
  toServerInfo,
  toUrlPreview,
} from './map'
import type { RawAttachment, RawChat, RawContact, RawHandle, RawMessage, RawServerInfo } from './map'

const DM_GUID = 'iMessage;-;+15555550100'
const GROUP_GUID = 'iMessage;+;chat123456789'

function rawHandle(overrides: Partial<RawHandle> = {}): RawHandle {
  return {
    originalROWID: 1,
    address: '+15555550100',
    service: 'iMessage',
    ...overrides,
  }
}

function rawMessage(overrides: Partial<RawMessage> = {}): RawMessage {
  return {
    originalROWID: 100,
    guid: 'message-guid-1',
    text: 'hello there',
    handle: rawHandle(),
    handleId: 1,
    subject: '',
    error: 0,
    dateCreated: 1_700_000_000_000,
    dateRead: null,
    dateDelivered: null,
    isFromMe: false,
    isArchived: false,
    itemType: 0,
    groupTitle: null,
    groupActionType: 0,
    balloonBundleId: null,
    associatedMessageGuid: null,
    associatedMessageType: null,
    expressiveSendStyleId: null,
    ...overrides,
  }
}

describe('toMessage', () => {
  test('maps a plain text message', () => {
    const raw = rawMessage({
      chats: [{ originalROWID: 1, guid: DM_GUID, style: 45, chatIdentifier: '+15555550100', isArchived: false, displayName: '' }],
    })

    const message = toMessage(raw, undefined, { contacts: new ContactIndex() })

    expect(message.guid).toBe('message-guid-1')
    expect(message.chatGuid).toBe(DM_GUID)
    expect(message.text).toBe('hello there')
    expect(message.fromMe).toBe(false)
    expect(message.service).toBe('iMessage')
    expect(message.sender?.address).toBe('+15555550100')
    expect(message.reaction).toBeUndefined()
    expect(message.groupEvent).toBeUndefined()
    expect(message.tapbacks).toEqual([])
  })

  test('falls back to the caller-provided chatGuid when chats is absent', () => {
    const raw = rawMessage()
    const message = toMessage(raw, DM_GUID)
    expect(message.chatGuid).toBe(DM_GUID)
  })

  test('maps a tapback add', () => {
    const raw = rawMessage({
      guid: 'reaction-guid-1',
      text: '',
      associatedMessageGuid: 'p:0/message-guid-1',
      associatedMessageType: 'love',
    })

    const message = toMessage(raw, DM_GUID)

    expect(message.reaction).toEqual({
      targetGuid: 'message-guid-1',
      kind: 'love',
      removed: false,
    })
  })

  test('maps a tapback removal with the bp: guid prefix', () => {
    const raw = rawMessage({
      guid: 'reaction-guid-2',
      text: '',
      associatedMessageGuid: 'bp:message-guid-1',
      associatedMessageType: '-love',
    })

    const message = toMessage(raw, DM_GUID)

    expect(message.reaction).toEqual({
      targetGuid: 'message-guid-1',
      kind: 'love',
      removed: true,
    })
  })

  test('maps a reply via threadOriginatorGuid, stripping the part prefix', () => {
    const raw = rawMessage({
      guid: 'reply-guid-1',
      text: 'sounds good',
      threadOriginatorGuid: 'p:0/original-guid-1',
    })

    const message = toMessage(raw, DM_GUID)

    expect(message.replyTo).toBe('original-guid-1')
  })

  test('maps a group rename event', () => {
    const raw = rawMessage({
      guid: 'rename-guid-1',
      text: '',
      handle: rawHandle({ address: '+15555550101' }),
      itemType: 2,
      groupTitle: 'Weekend Trip',
      chats: [{ originalROWID: 2, guid: GROUP_GUID, style: 43, chatIdentifier: 'chat123456789', isArchived: false, displayName: 'Weekend Trip' }],
    })

    const message = toMessage(raw)

    expect(message.groupEvent).toEqual({ kind: 'rename', title: 'Weekend Trip' })
    expect(message.chatGuid).toBe(GROUP_GUID)
  })

  test('maps a participant-left event to a leave group event', () => {
    const raw = rawMessage({
      guid: 'leave-guid-1',
      text: '',
      handle: rawHandle({ address: '+15555550102' }),
      itemType: 1,
      groupActionType: 1,
    })

    const message = toMessage(raw, GROUP_GUID)

    expect(message.groupEvent?.kind).toBe('leave')
    expect(message.groupEvent?.kind === 'leave' && message.groupEvent.who?.address).toBe('+15555550102')
  })

  test('maps an attachment message', () => {
    const raw = rawMessage({
      guid: 'attachment-guid-1',
      text: '',
      attachments: [
        {
          originalROWID: 5,
          guid: 'att-guid-1',
          uti: 'public.jpeg',
          mimeType: 'image/jpeg',
          totalBytes: 204_800,
          transferName: 'IMG_0001.jpeg',
          width: 100,
          height: 100,
        },
      ],
    })

    const attachmentPaths = new Map([['att-guid-1', '/attachments/att-guid-1.jpeg']])
    const message = toMessage(raw, DM_GUID, { attachmentPaths })

    expect(message.attachments).toHaveLength(1)
    expect(message.attachments[0]).toMatchObject({
      guid: 'att-guid-1',
      name: 'IMG_0001.jpeg',
      mime: 'image/jpeg',
      bytes: 204_800,
      isSticker: false,
      localPath: '/attachments/att-guid-1.jpeg',
    })
  })

  test('resolves the sender name from the contact index', () => {
    const contacts = new ContactIndex([
      { id: '1', name: 'Alex Rivera', addresses: ['+1 (555) 555-0100'] },
    ])
    const raw = rawMessage({ handle: rawHandle({ address: '+15555550100' }) })

    const message = toMessage(raw, DM_GUID, { contacts })

    expect(message.sender?.name).toBe('Alex Rivera')
  })

  test('reports a delivery error as a human string', () => {
    const raw = rawMessage({ error: 22 })
    const message = toMessage(raw, DM_GUID)
    expect(message.error).toBe('Not delivered (error 22)')
  })
})

describe('toChat', () => {
  test('maps a DM chat', () => {
    const raw: RawChat = {
      originalROWID: 1,
      guid: DM_GUID,
      style: 45,
      chatIdentifier: '+15555550100',
      isArchived: false,
      displayName: '',
      participants: [rawHandle()],
    }

    const chat = toChat(raw)

    expect(chat.isGroup).toBe(false)
    expect(chat.service).toBe('iMessage')
    expect(chat.participants).toHaveLength(1)
    expect(chat.displayName).toBeUndefined()
  })

  test('maps a group chat', () => {
    const raw: RawChat = {
      originalROWID: 2,
      guid: GROUP_GUID,
      style: 43,
      chatIdentifier: 'chat123456789',
      isArchived: false,
      displayName: 'Weekend Trip',
      participants: [rawHandle({ address: '+15555550100' }), rawHandle({ address: '+15555550101' })],
    }

    const chat = toChat(raw)

    expect(chat.isGroup).toBe(true)
    expect(chat.displayName).toBe('Weekend Trip')
    expect(chat.participants).toHaveLength(2)
  })

  test('marks a chat unread when the last message is incoming and unread', () => {
    const raw: RawChat = {
      originalROWID: 3,
      guid: DM_GUID,
      style: 45,
      chatIdentifier: '+15555550100',
      isArchived: false,
      displayName: '',
      lastMessage: rawMessage({ dateRead: null, isFromMe: false }),
    }

    const chat = toChat(raw)

    expect(chat.unread).toBe(true)
    expect(chat.lastActivity).toBe(chat.lastMessage?.date)
  })

  test('marks a chat read when the last message is our own', () => {
    const raw: RawChat = {
      originalROWID: 4,
      guid: DM_GUID,
      style: 45,
      chatIdentifier: '+15555550100',
      isArchived: false,
      displayName: '',
      lastMessage: rawMessage({ dateRead: null, isFromMe: true }),
    }

    const chat = toChat(raw)

    expect(chat.unread).toBe(false)
  })
})

describe('toServerInfo', () => {
  test('maps the server metadata response', () => {
    const raw: RawServerInfo = {
      os_version: '14.5',
      server_version: '1.9.4',
      private_api: true,
      helper_connected: true,
      detected_icloud: 'user@icloud.com',
    }

    expect(toServerInfo(raw)).toEqual({
      version: '1.9.4',
      macosVersion: '14.5',
      privateApi: true,
      helperConnected: true,
      icloudAccount: 'user@icloud.com',
    })
  })

  test('omits an empty icloud account', () => {
    const raw: RawServerInfo = {
      os_version: '14.5',
      server_version: '1.9.4',
      private_api: false,
      helper_connected: false,
      detected_icloud: '',
    }

    expect(toServerInfo(raw).icloudAccount).toBeUndefined()
  })
})

describe('toHandle', () => {
  test('maps service and resolves a contact name', () => {
    const contacts = new ContactIndex([{ id: '1', name: 'Sam Lee', addresses: ['sam@example.com'] }])
    const handle = toHandle(rawHandle({ address: 'SAM@Example.com', service: 'iMessage' }), contacts)
    expect(handle.service).toBe('iMessage')
    expect(handle.name).toBe('Sam Lee')
  })

  test('maps SMS and RCS services', () => {
    expect(toHandle(rawHandle({ service: 'SMS' })).service).toBe('SMS')
    expect(toHandle(rawHandle({ service: 'RCS' })).service).toBe('RCS')
  })
})

describe('toContact', () => {
  test('maps phone numbers and emails into a flat address list', () => {
    const raw: RawContact = {
      id: 42,
      firstName: 'Jordan',
      lastName: 'Blake',
      phoneNumbers: [{ address: '+15555550100', id: '1' }],
      emails: [{ address: 'jordan@example.com', id: '2' }],
      avatar: '',
    }

    const contact = toContact(raw)

    expect(contact.id).toBe('42')
    expect(contact.name).toBe('Jordan Blake')
    expect(contact.addresses).toEqual(['+15555550100', 'jordan@example.com'])
    expect(contact.avatar).toBeUndefined()
  })

  test('builds a data URL from a base64 avatar', () => {
    const raw: RawContact = {
      id: '1',
      displayName: 'Riley',
      phoneNumbers: [],
      emails: [],
      avatar: 'aGVsbG8=',
    }

    expect(toContact(raw).avatar).toBe('data:image/jpeg;base64,aGVsbG8=')
  })

  test('prefers a given avatarPath over the base64 payload', () => {
    const raw: RawContact = {
      id: '1',
      displayName: 'Riley',
      phoneNumbers: [],
      emails: [],
      avatar: 'aGVsbG8=',
    }

    expect(toContact(raw, '/cache/avatars/contact-1.jpg').avatar).toBe('/cache/avatars/contact-1.jpg')
  })
})

describe('ContactIndex', () => {
  test('normalizes phone numbers across formatting differences', () => {
    const contacts = new ContactIndex([{ id: '1', name: 'Casey', addresses: ['+1 (555) 555-0100'] }])
    expect(contacts.resolve('5555550100')).toBe('Casey')
    expect(contacts.resolve('15555550100')).toBe('Casey')
    expect(contacts.resolve('+15555550100')).toBe('Casey')
  })

  test('lowercases emails', () => {
    const contacts = new ContactIndex([{ id: '1', name: 'Drew', addresses: ['Drew@Example.com'] }])
    expect(contacts.resolve('drew@example.com')).toBe('Drew')
  })

  test('returns undefined for an unknown address', () => {
    const contacts = new ContactIndex([{ id: '1', name: 'Casey', addresses: ['+15555550100'] }])
    expect(contacts.resolve('+15555559999')).toBeUndefined()
  })

  test('resolves an avatar with the same address normalisation as the name', () => {
    const contacts = new ContactIndex([
      { id: '1', name: 'Casey', addresses: ['+1 (555) 555-0100'], avatar: '/cache/avatars/contact-1.jpg' },
      { id: '2', name: 'Drew', addresses: ['drew@example.com'] },
    ])
    expect(contacts.avatar('5555550100')).toBe('/cache/avatars/contact-1.jpg')
    expect(contacts.avatar('drew@example.com')).toBeUndefined()
    expect(contacts.avatar('+15555559999')).toBeUndefined()
  })
})

describe('chat and message service resolution', () => {
  test('derives an any; chat service from participants when none are iMessage', () => {
    const raw: RawChat = {
      originalROWID: 20,
      guid: 'any;-;+15555550199',
      style: 45,
      chatIdentifier: '+15555550199',
      isArchived: false,
      displayName: '',
      participants: [rawHandle({ address: '+15555550199', service: 'SMS' })],
    }
    expect(toChat(raw).service).toBe('SMS')
  })

  test('prefers iMessage over RCS/SMS participants for an any; chat', () => {
    const raw: RawChat = {
      originalROWID: 21,
      guid: 'any;+;chat9999',
      style: 43,
      chatIdentifier: 'chat9999',
      isArchived: false,
      displayName: '',
      participants: [rawHandle({ address: '+1', service: 'RCS' }), rawHandle({ address: '+2', service: 'iMessage' })],
    }
    expect(toChat(raw).service).toBe('iMessage')
  })

  test('falls back to iMessage for an any; chat with no participants', () => {
    const raw: RawChat = {
      originalROWID: 22,
      guid: 'any;-;+1',
      style: 45,
      chatIdentifier: '+1',
      isArchived: false,
      displayName: '',
    }
    expect(toChat(raw).service).toBe('iMessage')
  })

  test('uses the chatService option for a sent message with no handle', () => {
    const raw = rawMessage({ handle: null, isFromMe: true })
    const message = toMessage(raw, 'any;-;+15555550199', { chatService: 'RCS' })
    expect(message.service).toBe('RCS')
  })

  test('falls back to the guid prefix for a sent message with no chatService option', () => {
    const raw = rawMessage({ handle: null, isFromMe: true })
    const message = toMessage(raw, 'SMS;-;+15555550199')
    expect(message.service).toBe('SMS')
  })
})

describe('downloadPlan', () => {
  test('requests a converted jpeg for a HEIC attachment', () => {
    expect(downloadPlan('IMG_0001.HEIC', 'image/heic')).toEqual({ original: false, extension: '.jpg' })
  })

  test('requests a converted m4a for a CAF audio attachment', () => {
    expect(downloadPlan('Audio Message.caf', 'audio/x-caf')).toEqual({ original: false, extension: '.m4a' })
  })

  test('keeps a plain png as-is', () => {
    expect(downloadPlan('photo.png', 'image/png')).toEqual({ original: false, extension: '.png' })
  })

  test('requests the original for a video attachment', () => {
    expect(downloadPlan('clip.mp4', 'video/mp4')).toEqual({ original: true, extension: '.mp4' })
  })

  test('handles an attachment with no name or mime', () => {
    expect(downloadPlan(undefined, undefined)).toEqual({ original: true, extension: '' })
  })
})

/** Built from the real payloadData sample, with an added summary and image substitute index. */
function richLinkArchive() {
  const objects: unknown[] = [
    '$null', // 0
    { richLinkIsPlaceholder: false, richLinkMetadata: { UID: 2 }, $class: { UID: 12 } }, // 1 root
    {
      $class: { UID: 13 },
      originalURL: { UID: 3 },
      title: { UID: 7 },
      summary: { UID: 8 },
      siteName: { UID: 9 },
      image: { UID: 10 },
    }, // 2 richLinkMetadata
    { 'NS.base': { UID: 0 }, $class: { UID: 5 }, 'NS.relative': { UID: 4 } }, // 3 NSURL
    'https://example.com/article', // 4
    { $classname: 'NSURL', $classes: ['NSURL', 'NSObject'] }, // 5
    null, // 6 unused
    'A great article', // 7 title
    'The article summary.', // 8 summary
    'Example Site', // 9 siteName
    { richLinkImageAttachmentSubstituteIndex: 0, $class: { UID: 11 }, MIMEType: 'image/png' }, // 10 image
    { $classname: 'RLImageMetadata', $classes: ['RLImageMetadata', 'NSObject'] }, // 11
    { $classname: 'NSDictionary' }, // 12
    { $classname: 'NSRichLinkMetadata' }, // 13
  ]
  return { $version: 100000, $archiver: 'NSKeyedArchiver', $top: { root: { UID: 1 } }, $objects: objects }
}

describe('decodeKeyedArchive', () => {
  test('resolves UID references, drops $class, and collapses NSURL wrappers', () => {
    const decoded = decodeKeyedArchive(richLinkArchive())
    expect(decoded).toEqual({
      richLinkIsPlaceholder: false,
      richLinkMetadata: {
        originalURL: 'https://example.com/article',
        title: 'A great article',
        summary: 'The article summary.',
        siteName: 'Example Site',
        image: { richLinkImageAttachmentSubstituteIndex: 0, MIMEType: 'image/png' },
      },
    })
  })
})

describe('toUrlPreview', () => {
  test('extracts url, title, summary and the substitute image guid', () => {
    const attachments: RawAttachment[] = [
      {
        originalROWID: 1,
        guid: 'preview-image-guid',
        uti: 'dyn.age8u',
        mimeType: '',
        totalBytes: 0,
        transferName: '',
        hideAttachment: true,
      },
    ]

    const preview = toUrlPreview([richLinkArchive()], attachments)

    expect(preview).toMatchObject({
      url: 'https://example.com/article',
      title: 'A great article',
      summary: 'The article summary.',
      siteName: 'Example Site',
      imageAttachmentGuid: 'preview-image-guid',
    })
  })

  test('sets urlPreview on a message with the URL balloon bundle id', () => {
    const raw = rawMessage({
      balloonBundleId: 'com.apple.messages.URLBalloonProvider',
      payloadData: [richLinkArchive()],
    })

    const message = toMessage(raw, DM_GUID)

    expect(message.urlPreview?.url).toBe('https://example.com/article')
    expect(message.text).toBe('hello there')
  })
})

describe('stickers', () => {
  test('maps a sticker as stickerFor, not a reaction, keeping its attachment', () => {
    const raw = rawMessage({
      guid: 'sticker-guid-1',
      text: '',
      associatedMessageGuid: 'p:0/message-guid-1',
      associatedMessageType: 'sticker',
      attachments: [
        {
          originalROWID: 9,
          guid: 'sticker-att-1',
          uti: 'com.apple.sticker',
          mimeType: 'image/png',
          totalBytes: 1024,
          transferName: 'sticker.png',
          isSticker: true,
        },
      ],
    })

    const message = toMessage(raw, DM_GUID)

    expect(message.reaction).toBeUndefined()
    expect(message.stickerFor).toBe('message-guid-1')
    expect(message.attachments).toHaveLength(1)
  })

  test('maps the numeric "1000" form the same way', () => {
    const raw = rawMessage({ associatedMessageGuid: 'bp:message-guid-1', associatedMessageType: '1000' })
    expect(toMessage(raw, DM_GUID).stickerFor).toBe('message-guid-1')
  })
})

describe('custom emoji tapbacks', () => {
  test('maps a known numeric add type to its named kind', () => {
    const raw = rawMessage({ associatedMessageGuid: 'p:0/message-guid-1', associatedMessageType: '2003' })
    expect(toMessage(raw, DM_GUID).reaction).toEqual({ targetGuid: 'message-guid-1', kind: 'laugh', removed: false })
  })

  test('extracts the emoji from the message text on an unrecognized add type', () => {
    const raw = rawMessage({
      text: 'Reacted 🔥 to “hello there”',
      associatedMessageGuid: 'p:0/message-guid-1',
      associatedMessageType: '2006',
    })
    expect(toMessage(raw, DM_GUID).reaction).toEqual({
      targetGuid: 'message-guid-1',
      kind: 'emoji',
      emoji: '🔥',
      removed: false,
    })
  })

  test('extracts the emoji from the message text on an unrecognized remove type', () => {
    const raw = rawMessage({
      text: 'Removed a 🔥 reaction from “hello there”',
      associatedMessageGuid: 'p:0/message-guid-1',
      associatedMessageType: '3006',
    })
    expect(toMessage(raw, DM_GUID).reaction).toEqual({
      targetGuid: 'message-guid-1',
      kind: 'emoji',
      emoji: '🔥',
      removed: true,
    })
  })

  test('falls back to a heart when no emoji is found in the text', () => {
    const raw = rawMessage({
      text: 'Reacted to “hello there”',
      associatedMessageGuid: 'p:0/message-guid-1',
      associatedMessageType: '2006',
    })
    expect(toMessage(raw, DM_GUID).reaction?.emoji).toBe('❤️')
  })
})

describe('attachment metadata', () => {
  test('maps durationMs from seconds and the hidden flag', () => {
    const raw = rawMessage({
      attachments: [
        {
          originalROWID: 6,
          guid: 'audio-att-1',
          uti: 'com.apple.coreaudio-format',
          mimeType: 'audio/x-caf',
          totalBytes: 40_000,
          transferName: 'Audio Message.caf',
          hideAttachment: true,
          metadata: { duration: 12.5, bitRate: 128_000, sampleRate: 44_100, bytes: 40_000 },
        },
      ],
    })

    const message = toMessage(raw, DM_GUID)

    expect(message.attachments[0]).toMatchObject({ durationMs: 12_500, hidden: true })
  })

  test('defaults hidden to false and leaves durationMs unset', () => {
    const raw = rawMessage({
      attachments: [
        { originalROWID: 7, guid: 'img-att-1', uti: 'public.jpeg', mimeType: 'image/jpeg', totalBytes: 500, transferName: 'a.jpg' },
      ],
    })

    const message = toMessage(raw, DM_GUID)

    expect(message.attachments[0]?.hidden).toBe(false)
    expect(message.attachments[0]?.durationMs).toBeUndefined()
  })
})

describe('malformed text handling', () => {
  test('replaces a lone surrogate so the text round-trips through JSON', () => {
    const raw = rawMessage({ text: 'hello \uD800 world' })

    const message = toMessage(raw, DM_GUID)

    expect(message.text).toBe('hello \uD800 world'.toWellFormed())
    expect(() => JSON.stringify(message)).not.toThrow()
  })

  test('strips the attachment placeholder character from text', () => {
    const raw = rawMessage({
      text: '￼',
      attachments: [
        { originalROWID: 8, guid: 'img-att-2', uti: 'public.jpeg', mimeType: 'image/jpeg', totalBytes: 500, transferName: 'b.jpg' },
      ],
    })

    const message = toMessage(raw, DM_GUID)

    expect(message.text).toBe('')
  })
})

describe('toParts', () => {
  const PART = '__kIMMessagePartAttributeName'

  function body(string: string, runs: Array<{ range: [number, number]; attributes?: Record<string, unknown> }>) {
    return [{ string, runs }]
  }

  function rawAttachment(guid: string, mimeType = 'image/jpeg'): RawAttachment {
    return { originalROWID: 1, guid, uti: 'public.jpeg', mimeType, totalBytes: 1000, transferName: `${guid}.jpg` }
  }

  test('returns undefined for a body with one unstyled run', () => {
    expect(toParts(body('hello there', [{ range: [0, 11], attributes: { [PART]: 0 } }]))).toBeUndefined()
  })

  test('returns undefined when the body is missing or malformed', () => {
    expect(toParts(undefined)).toBeUndefined()
    expect(toParts([{ string: 'hello' }])).toBeUndefined()
    expect(toParts([{ string: 'hello', runs: [] }])).toBeUndefined()
    expect(toParts([{ runs: [{ range: [0, 5] }] }])).toBeUndefined()
    expect(toParts(body('hello', [{ range: [0, 5] } as never, { attributes: { [PART]: 0 } } as never]))).toBeUndefined()
  })

  test('keeps bold and italic runs and merges the unstyled ones around them', () => {
    const parts = toParts(
      body('a bold and italic end', [
        { range: [0, 2], attributes: { [PART]: 0 } },
        { range: [2, 4], attributes: { [PART]: 0, __kIMTextBoldAttributeName: 1 } },
        { range: [6, 5], attributes: { [PART]: 0 } },
        { range: [11, 6], attributes: { [PART]: 0, __kIMTextItalicAttributeName: 1 } },
        { range: [17, 4], attributes: { [PART]: 0 } },
      ]),
    )

    expect(parts).toEqual([
      {
        kind: 'text',
        runs: [
          { text: 'a ' },
          { text: 'bold', bold: true },
          { text: ' and ' },
          { text: 'italic', italic: true },
          { text: ' end' },
        ],
      },
    ])
  })

  test('marks underline, strikethrough and a text effect', () => {
    const parts = toParts(
      body('under struck big', [
        { range: [0, 6], attributes: { [PART]: 0, __kIMTextUnderlineAttributeName: 1 } },
        { range: [6, 7], attributes: { [PART]: 0, __kIMTextStrikethroughAttributeName: 1 } },
        { range: [13, 3], attributes: { [PART]: 0, __kIMTextEffectAttributeName: 5 } },
      ]),
    )

    expect(parts?.[0]).toMatchObject({
      kind: 'text',
      runs: [{ underline: true }, { strike: true }, { text: 'big', effect: 'big' }],
    })
  })

  test('carries a mention address and the display name it covers', () => {
    const parts = toParts(
      body('hey Sam, ready?', [
        { range: [0, 4], attributes: { [PART]: 0 } },
        { range: [4, 3], attributes: { [PART]: 0, __kIMMentionConfirmedMention: '+14155550103' } },
        { range: [7, 8], attributes: { [PART]: 0 } },
      ]),
    )

    expect(parts?.[0]).toEqual({
      kind: 'text',
      runs: [{ text: 'hey ' }, { text: 'Sam', mention: '+14155550103' }, { text: ', ready?' }],
    })
  })

  test('takes the href from the link attribute rather than the run text', () => {
    const parts = toParts(
      body('read the docs', [
        { range: [0, 9], attributes: { [PART]: 0 } },
        { range: [9, 4], attributes: { [PART]: 0, __kIMLinkAttributeName: 'https://docs.bluebubbles.app' } },
      ]),
    )

    expect(parts?.[0]).toEqual({
      kind: 'text',
      runs: [{ text: 'read the ' }, { text: 'docs', link: 'https://docs.bluebubbles.app' }],
    })
  })

  test('places an attachment between the two lines of text around it', () => {
    const parts = toParts(
      body('before\n￼\nafter', [
        { range: [0, 7], attributes: { [PART]: 0 } },
        { range: [7, 1], attributes: { [PART]: 1, __kIMFileTransferGUIDAttributeName: 'att-1' } },
        { range: [8, 6], attributes: { [PART]: 2 } },
      ]),
      [rawAttachment('att-1')],
    )

    expect(parts).toEqual([
      { kind: 'text', runs: [{ text: 'before' }] },
      { kind: 'attachment', guid: 'att-1' },
      { kind: 'text', runs: [{ text: 'after' }] },
    ])
  })

  test('drops a placeholder for an attachment the query did not return', () => {
    const parts = toParts(
      body('look\n￼', [
        { range: [0, 5], attributes: { [PART]: 0, __kIMTextBoldAttributeName: 1 } },
        { range: [5, 1], attributes: { [PART]: 1, __kIMFileTransferGUIDAttributeName: 'missing' } },
      ]),
    )

    expect(parts).toEqual([{ kind: 'text', runs: [{ text: 'look', bold: true }] }])
  })

  test('splits a two-part message on the part index', () => {
    const parts = toParts(
      body('first\nsecond', [
        { range: [0, 6], attributes: { [PART]: 0 } },
        { range: [6, 6], attributes: { [PART]: 1 } },
      ]),
    )

    expect(parts).toEqual([
      { kind: 'text', runs: [{ text: 'first' }] },
      { kind: 'text', runs: [{ text: 'second' }] },
    ])
  })

  test('toMessage sets parts from attributedBody and leaves plain messages alone', () => {
    const styled = toMessage(
      rawMessage({
        text: 'hello there',
        attributedBody: body('hello there', [
          { range: [0, 5], attributes: { [PART]: 0, __kIMTextBoldAttributeName: 1 } },
          { range: [5, 6], attributes: { [PART]: 0 } },
        ]),
      }),
      DM_GUID,
    )

    expect(styled.parts).toEqual([{ kind: 'text', runs: [{ text: 'hello', bold: true }, { text: ' there' }] }])
    expect(styled.text).toBe('hello there')
    expect(toMessage(rawMessage(), DM_GUID).parts).toBeUndefined()
  })
})

describe('ContactIndex without country codes', () => {
  test('resolves a +34 handle against a contact stored with nine local digits', () => {
    const index = new ContactIndex([{ id: '1', name: 'Keara', addresses: ['699 68 62 42'] }])
    expect(index.resolve('+34699686242')).toBe('Keara')
    expect(index.resolve('0034699686242')).toBe('Keara')
    expect(index.resolve('+34600000000')).toBeUndefined()
  })
})
