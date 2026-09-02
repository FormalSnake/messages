import type {
  Attachment,
  Chat,
  Contact,
  GroupEvent,
  Handle,
  Message,
  Reaction,
  ServerInfo,
  Service,
  TapbackKind,
} from '../model'

export interface RawHandle {
  originalROWID: number
  address: string
  service: string
  country?: string
  uncanonicalizedId?: string
}

export interface RawAttachment {
  originalROWID: number
  guid: string
  blurhash?: string
  height?: number
  width?: number
  uti: string
  mimeType: string
  totalBytes: number
  transferName: string
  isSticker?: boolean
  hideAttachment?: boolean
  hasLivePhoto?: boolean
}

export interface RawChat {
  originalROWID: number
  guid: string
  participants?: RawHandle[]
  lastMessage?: RawMessage
  properties?: unknown
  style: number
  chatIdentifier: string
  isArchived: boolean
  displayName: string
  groupId?: string
}

export interface RawMessage {
  originalROWID: number
  tempGuid?: string
  guid: string
  text: string
  attributedBody?: unknown
  handle?: RawHandle | null
  handleId: number
  chats?: RawChat[]
  attachments?: RawAttachment[]
  subject: string
  error: number
  dateCreated: number
  dateRead: number | null
  dateDelivered: number | null
  isFromMe: boolean
  isDelivered?: boolean
  isArchived: boolean
  isAudioMessage?: boolean
  itemType: number
  groupTitle: string | null
  groupActionType: number
  balloonBundleId: string | null
  associatedMessageGuid: string | null
  /**
   * Already decoded to a name by the server, never numeric: "love" | "like" |
   * "dislike" | "laugh" | "emphasize" | "question" (add), the same six
   * prefixed with "-" (remove), "sticker", or the raw DB integer as a string
   * for anything else. There is no custom-emoji variant and no separate
   * emoji field anywhere in the response.
   * packages/server/src/server/databases/transformers/MessageTypeTransformer.ts
   * packages/server/src/server/api/serializers/MessageSerializer.ts
   */
  associatedMessageType: string | null
  expressiveSendStyleId: string | null
  threadOriginatorGuid?: string | null
  threadOriginatorPart?: string | null
  dateRetracted?: number | null
  dateEdited?: number | null
  partCount?: number | null
  payloadData?: unknown
}

export interface RawServerInfo {
  os_version: string
  server_version: string
  private_api: boolean
  helper_connected: boolean
  detected_icloud?: string
}

/** packages/server/src/server/api/interfaces/contactInterface.ts ContactInterface.mapContacts */
export interface RawContactAddress {
  address: string
  id?: string | number | null
}

export interface RawContact {
  id: string | number
  firstName?: string
  lastName?: string
  displayName?: string
  nickname?: string
  phoneNumbers: RawContactAddress[]
  emails: RawContactAddress[]
  avatar: string
}

export interface MapOptions {
  contacts?: ContactIndex
  /** guid -> local file path, precomputed by the transport since map.ts stays sync. */
  attachmentPaths?: Map<string, string>
}

function toService(service: string): Service {
  if (service === 'SMS') return 'SMS'
  if (service === 'RCS') return 'RCS'
  return 'iMessage'
}

/** Chat/message guids are prefixed "iMessage;-;...", "SMS;-;..." or "RCS;-;...". */
function chatService(chatGuid: string): Service {
  if (chatGuid.startsWith('SMS;')) return 'SMS'
  if (chatGuid.startsWith('RCS;')) return 'RCS'
  return 'iMessage'
}

/**
 * chat.db stores reaction/reply targets as "p:<partIndex>/<guid>" (a specific
 * message part) or "bp:<guid>" (the whole message body). The server passes
 * associatedMessageGuid and threadOriginatorGuid through unchanged.
 */
function stripGuidPrefix(guid: string): string {
  const partMatch = /^p:\d+\//.exec(guid)
  if (partMatch) return guid.slice(partMatch[0].length)
  if (guid.startsWith('bp:')) return guid.slice(3)
  return guid
}

const REACTION_KIND: Record<string, TapbackKind> = {
  love: 'love',
  like: 'like',
  dislike: 'dislike',
  laugh: 'laugh',
  emphasize: 'emphasize',
  question: 'question',
}

function parseReaction(raw: RawMessage): Reaction | undefined {
  const type = raw.associatedMessageType
  if (!type || !raw.associatedMessageGuid) return undefined
  const removed = type.startsWith('-')
  const kind = REACTION_KIND[removed ? type.slice(1) : type]
  if (!kind) return undefined
  return {
    targetGuid: stripGuidPrefix(raw.associatedMessageGuid),
    kind,
    removed,
  }
}

function parseGroupEvent(raw: RawMessage, who: Handle | undefined): GroupEvent | undefined {
  if (raw.itemType === 1) {
    return raw.groupActionType === 1 ? { kind: 'leave', who } : { kind: 'join', who }
  }
  if (raw.itemType === 2) {
    return { kind: 'rename', title: raw.groupTitle ?? '' }
  }
  if (raw.itemType === 3) {
    return raw.groupActionType === 1 ? { kind: 'photo' } : { kind: 'leave', who }
  }
  return undefined
}

export function toHandle(raw: RawHandle, contacts?: ContactIndex): Handle {
  return {
    address: raw.address,
    service: toService(raw.service),
    name: contacts?.resolve(raw.address),
  }
}

export function toAttachment(raw: RawAttachment, localPath?: string): Attachment {
  return {
    guid: raw.guid,
    name: raw.transferName,
    mime: raw.mimeType,
    bytes: raw.totalBytes,
    width: raw.width,
    height: raw.height,
    isSticker: raw.isSticker ?? false,
    localPath,
  }
}

export function toMessage(raw: RawMessage, chatGuid?: string, options: MapOptions = {}): Message {
  const resolvedChatGuid = raw.chats?.[0]?.guid ?? chatGuid ?? ''
  const sender = raw.handle ? toHandle(raw.handle, options.contacts) : undefined

  return {
    guid: raw.guid,
    tempGuid: raw.tempGuid,
    chatGuid: resolvedChatGuid,
    text: raw.text ?? '',
    subject: raw.subject || undefined,
    fromMe: raw.isFromMe,
    sender,
    date: raw.dateCreated,
    dateDelivered: raw.dateDelivered ?? undefined,
    dateRead: raw.dateRead ?? undefined,
    dateEdited: raw.dateEdited ?? undefined,
    dateRetracted: raw.dateRetracted ?? undefined,
    service: chatService(resolvedChatGuid),
    attachments: (raw.attachments ?? []).map(a => toAttachment(a, options.attachmentPaths?.get(a.guid))),
    tapbacks: [],
    replyTo: raw.threadOriginatorGuid ? stripGuidPrefix(raw.threadOriginatorGuid) : undefined,
    effect: raw.expressiveSendStyleId ?? undefined,
    error: raw.error ? `Not delivered (error ${raw.error})` : undefined,
    isAudio: raw.isAudioMessage ?? false,
    groupEvent: raw.itemType !== 0 ? parseGroupEvent(raw, sender) : undefined,
    reaction: parseReaction(raw),
    balloonBundleId: raw.balloonBundleId ?? undefined,
  }
}

export function toChat(raw: RawChat, options: MapOptions = {}): Chat {
  const lastMessage = raw.lastMessage ? toMessage(raw.lastMessage, raw.guid, options) : undefined
  return {
    guid: raw.guid,
    identifier: raw.chatIdentifier,
    service: chatService(raw.guid),
    isGroup: raw.style === 43,
    displayName: raw.displayName || undefined,
    participants: (raw.participants ?? []).map(h => toHandle(h, options.contacts)),
    pinned: false,
    muted: false,
    archived: raw.isArchived,
    // chat.db has no reliable per-chat unread column; approximate it from
    // whether the last message is unread and not our own.
    unread: Boolean(lastMessage && !lastMessage.fromMe && !lastMessage.dateRead),
    lastMessage,
    lastActivity: lastMessage?.date ?? 0,
  }
}

export function toServerInfo(raw: RawServerInfo): ServerInfo {
  return {
    version: raw.server_version,
    macosVersion: raw.os_version,
    privateApi: raw.private_api,
    helperConnected: raw.helper_connected,
    icloudAccount: raw.detected_icloud || undefined,
  }
}

export function toContact(raw: RawContact): Contact {
  const addresses = [...raw.phoneNumbers, ...raw.emails]
    .map(a => a.address)
    .filter((a): a is string => Boolean(a))

  let name = raw.displayName
  if (!name) {
    if (raw.firstName && raw.lastName) name = `${raw.firstName} ${raw.lastName}`
    else if (raw.firstName) name = raw.firstName
    else if (raw.nickname) name = raw.nickname
  }

  return {
    id: String(raw.id),
    name: name || addresses[0] || '',
    addresses,
    // The server returns a bare base64 string (no data: prefix) with no mime
    // hint; Apple Contacts thumbnails are JPEG in practice.
    avatar: raw.avatar ? `data:image/jpeg;base64,${raw.avatar}` : undefined,
  }
}

function normalizeDigits(address: string): string {
  return address.replace(/[^\d]/g, '')
}

export class ContactIndex {
  private readonly byDigits = new Map<string, string>()
  private readonly byLast10 = new Map<string, string>()
  private readonly byEmail = new Map<string, string>()

  constructor(contacts: Contact[] = []) {
    for (const contact of contacts) {
      for (const address of contact.addresses) {
        if (address.includes('@')) {
          this.byEmail.set(address.toLowerCase(), contact.name)
          continue
        }
        const digits = normalizeDigits(address)
        if (!digits) continue
        this.byDigits.set(digits, contact.name)
        // Match on the last 10 digits too, so a stored "+15555550123" resolves
        // an incoming "5555550123" or "15555550123" and vice versa.
        if (digits.length >= 10) this.byLast10.set(digits.slice(-10), contact.name)
      }
    }
  }

  resolve(address: string): string | undefined {
    if (address.includes('@')) return this.byEmail.get(address.toLowerCase())
    const digits = normalizeDigits(address)
    if (!digits) return undefined
    const exact = this.byDigits.get(digits)
    if (exact) return exact
    if (digits.length >= 10) return this.byLast10.get(digits.slice(-10))
    return undefined
  }
}
