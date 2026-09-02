export type Service = 'iMessage' | 'SMS' | 'RCS'

export interface Handle {
  /** E.164 phone number or email address. */
  address: string
  service: Service
  /** Resolved contact name, when the server knows one. */
  name?: string
  /** Local file path or data URL. */
  avatar?: string
}

export interface Contact {
  id: string
  name: string
  addresses: string[]
  avatar?: string
}

export type TapbackKind = 'love' | 'like' | 'dislike' | 'laugh' | 'emphasize' | 'question' | 'emoji'

export interface Tapback {
  /** Guid of the reaction message itself, so a removal can find it. */
  guid: string
  kind: TapbackKind
  /** Set for custom emoji tapbacks. */
  emoji?: string
  fromMe: boolean
  sender?: Handle
}

export interface Attachment {
  guid: string
  name: string
  mime: string
  bytes: number
  width?: number
  height?: number
  isSticker: boolean
  /** Path inside the attachment cache once downloaded. */
  localPath?: string
  /** Set when the server hid this attachment, e.g. a rich-link preview image. */
  hidden: boolean
  /** Audio message duration in milliseconds, when the server reports one. */
  durationMs?: number
}

/** iOS 18 text effects, from __kIMTextEffectAttributeName. */
export type TextEffect = 'big' | 'small' | 'shake' | 'nod' | 'explode' | 'ripple' | 'bloom' | 'jitter'

/** One stretch of a message body with a single set of attributes. */
export interface RichRun {
  text: string
  bold?: boolean
  italic?: boolean
  underline?: boolean
  strike?: boolean
  /** Href from __kIMLinkAttributeName, not from linkifying the text ourselves. */
  link?: string
  /** Address of the mentioned handle; the run text is the display name. */
  mention?: string
  effect?: TextEffect
}

/**
 * A message body split the way Messages splits it: text and attachments in the
 * order the attributed body puts them, so a photo can sit between two lines.
 */
export type MessagePart = { kind: 'text'; runs: RichRun[] } | { kind: 'attachment'; guid: string }

export type DeliveryState = 'sending' | 'sent' | 'delivered' | 'read' | 'failed'

export type GroupEvent =
  | { kind: 'rename'; title: string }
  | { kind: 'join'; who?: Handle }
  | { kind: 'leave'; who?: Handle }
  | { kind: 'photo' }

export interface Reaction {
  targetGuid: string
  kind: TapbackKind
  emoji?: string
  removed: boolean
}

export interface Message {
  guid: string
  /** Client-side guid used while a send is in flight. */
  tempGuid?: string
  chatGuid: string
  text: string
  subject?: string
  fromMe: boolean
  sender?: Handle
  date: number
  dateDelivered?: number
  dateRead?: number
  dateEdited?: number
  dateRetracted?: number
  service: Service
  attachments: Attachment[]
  tapbacks: Tapback[]
  /**
   * Set only when the attributed body carries formatting, a mention, a link
   * attribute, or attachment placement. Plain messages keep `text` alone.
   */
  parts?: MessagePart[]
  /** Guid of the message this one replies to. */
  replyTo?: string
  /** Guid of the message this sticker was placed on, when this message is a sticker. */
  stickerFor?: string
  /** Expressive send style, for example com.apple.MobileSMS.expressivesend.impact. */
  effect?: string
  error?: string
  isAudio: boolean
  groupEvent?: GroupEvent
  /**
   * Reactions arrive as messages of their own. The store folds them into the
   * target message's `tapbacks` and never shows them as rows.
   */
  reaction?: Reaction
  /** iMessage app payloads: Apple Pay, polls, link previews. */
  balloonBundleId?: string
  urlPreview?: {
    url: string
    title?: string
    summary?: string
    imagePath?: string
    imageAttachmentGuid?: string
    siteName?: string
  }
}

export interface Chat {
  guid: string
  identifier: string
  service: Service
  isGroup: boolean
  displayName?: string
  participants: Handle[]
  pinned: boolean
  muted: boolean
  archived: boolean
  unread: boolean
  lastMessage?: Message
  lastActivity: number
}

export interface ServerInfo {
  version: string
  macosVersion?: string
  /** Private API toggle on the server. Needs SIP disabled on the Mac. */
  privateApi: boolean
  /** The helper bundle is injected into Messages.app and talking to the server. */
  helperConnected: boolean
  icloudAccount?: string
}

export interface Capabilities {
  reactions: boolean
  typing: boolean
  readReceipts: boolean
  edit: boolean
  unsend: boolean
  replies: boolean
  effects: boolean
  groupManagement: boolean
  markUnread: boolean
  facetime: boolean
  scheduledMessages: boolean
}

export function capabilitiesFor(info: ServerInfo | null): Capabilities {
  const privateApi = Boolean(info?.privateApi && info.helperConnected)
  return {
    reactions: privateApi,
    typing: privateApi,
    readReceipts: privateApi,
    edit: privateApi,
    unsend: privateApi,
    replies: privateApi,
    effects: privateApi,
    groupManagement: privateApi,
    markUnread: privateApi,
    facetime: privateApi,
    scheduledMessages: Boolean(info),
  }
}

export function handleName(handle: Handle): string {
  return handle.name?.trim() || handle.address
}

export function chatTitle(chat: Chat): string {
  if (chat.displayName?.trim()) return chat.displayName.trim()
  const names = chat.participants.map(handleName)
  if (names.length === 0) return chat.identifier
  if (names.length <= 3) return names.join(', ')
  return `${names.slice(0, 3).join(', ')} and ${names.length - 3} more`
}

export function deliveryState(message: Message): DeliveryState {
  if (message.error) return 'failed'
  if (message.tempGuid && message.guid === message.tempGuid) return 'sending'
  if (message.dateRead) return 'read'
  if (message.dateDelivered) return 'delivered'
  return 'sent'
}

export function isVisibleMessage(message: Message): boolean {
  return !message.reaction
}

export const TAPBACK_GLYPH: Record<Exclude<TapbackKind, 'emoji'>, string> = {
  love: '❤️',
  like: '👍',
  dislike: '👎',
  laugh: '😂',
  emphasize: '‼️',
  question: '❓',
}

export function tapbackGlyph(tapback: Pick<Tapback, 'kind' | 'emoji'>): string {
  if (tapback.kind === 'emoji') return tapback.emoji ?? '❤️'
  return TAPBACK_GLYPH[tapback.kind]
}
