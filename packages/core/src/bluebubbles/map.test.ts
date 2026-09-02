import { describe, expect, test } from 'vitest'
import { ContactIndex, toChat, toContact, toHandle, toMessage, toServerInfo } from './map'
import type { RawChat, RawContact, RawHandle, RawMessage, RawServerInfo } from './map'

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
})
