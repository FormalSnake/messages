import { extname } from 'node:path'
import type {
  Attachment,
  Chat,
  Contact,
  GroupEvent,
  Handle,
  Message,
  MessagePart,
  Reaction,
  RichRun,
  ServerInfo,
  Service,
  TapbackKind,
  TextEffect,
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
  /** duration is in seconds, from macOS mdls; only set on audio attachments. */
  metadata?: { duration?: number; bitRate?: number; sampleRate?: number; bytes?: number }
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
   * Already decoded to a name by the server for the six named tapbacks:
   * "love" | "like" | "dislike" | "laugh" | "emphasize" | "question" (add),
   * the same six prefixed with "-" (remove), or "sticker". Anything else
   * arrives as the raw chat.db integer, stringified: "1000" is also a
   * sticker, "2000"-"2005"/"3000"-"3005" are the same six tapbacks add/remove,
   * and any other "2xxx"/"3xxx" is a macOS 15+ custom emoji tapback the
   * server never decodes (see parseAssociatedMessage).
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
  /** Service for a message with no handle (sent by me), precomputed by the transport from the chat. */
  chatService?: Service
}

// tsconfig's lib list predates ES2024.String; Bun's runtime has had
// toWellFormed()/isWellFormed() since well before that, so declare it here
// rather than widen the lib for the whole package.
declare global {
  interface String {
    toWellFormed(): string
    isWellFormed(): boolean
  }
}

function toService(service: string): Service {
  if (service === 'SMS') return 'SMS'
  if (service === 'RCS') return 'RCS'
  return 'iMessage'
}

function wellFormed(value: string): string
function wellFormed(value: string | null): string | null
function wellFormed(value: string | undefined): string | undefined
function wellFormed(value: string | null | undefined): string | null | undefined {
  // A lone UTF-16 surrogate in server text (seen on live data) makes the
  // native JSON parser reject the whole payload downstream ("unexpected end
  // of hex escape"). toWellFormed() swaps it for U+FFFD before it can reach the UI.
  return value == null ? value : value.toWellFormed()
}

/**
 * Messages stores U+FFFC (object replacement character) in `text` where an
 * attachment sits, so an attachment-only message's text arrives as "\uFFFC".
 * Strip it and trim so such messages end up with text === '' instead of a
 * lone placeholder glyph.
 */
function cleanText(value: string): string {
  return wellFormed(value).replace(/\uFFFC/g, '').trim()
}

function prefixService(chatGuid: string): Service | undefined {
  const prefix = chatGuid.split(';', 1)[0]
  if (prefix === 'iMessage' || prefix === 'SMS' || prefix === 'RCS') return prefix
  return undefined
}

function serviceFromParticipants(participants?: RawHandle[]): Service {
  const services = new Set((participants ?? []).map(p => toService(p.service)))
  if (services.has('iMessage')) return 'iMessage'
  if (services.has('RCS')) return 'RCS'
  if (services.has('SMS')) return 'SMS'
  return 'iMessage'
}

/**
 * Chat/message guids are prefixed "iMessage;-;...", "SMS;-;..." or "RCS;-;..."
 * pre-macOS 26. On macOS 26 every chat guid is "any;-;..." regardless of
 * service, so there the service has to come from the participants instead.
 */
function chatService(chatGuid: string, participants?: RawHandle[]): Service {
  return prefixService(chatGuid) ?? serviceFromParticipants(participants)
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

// Numeric associatedMessageType bases: 2000-2005 (add) and 3000-3005 (remove)
// are the same six tapbacks in this order, one-for-one with REACTION_KIND.
const NUMERIC_REACTION_KIND: TapbackKind[] = ['love', 'like', 'dislike', 'laugh', 'emphasize', 'question']

// A custom emoji tapback (macOS 15+) is only ever visible in the message
// text, which Messages writes as "Reacted 🔥 to “…”" / "Removed a 🔥
// reaction from “…”" (wording varies by locale). Match the first emoji
// grapheme cluster rather than the surrounding words.
const EMOJI_CLUSTER = /\p{Extended_Pictographic}(?:\uFE0F|\p{Emoji_Modifier})?(?:\u200D\p{Extended_Pictographic}(?:\uFE0F|\p{Emoji_Modifier})?)*/u

function extractEmoji(text: string): string | undefined {
  return EMOJI_CLUSTER.exec(text)?.[0]
}

interface AssociatedMessage {
  reaction?: Reaction
  stickerFor?: string
}

function parseAssociatedMessage(raw: RawMessage): AssociatedMessage {
  const type = raw.associatedMessageType
  if (!type || !raw.associatedMessageGuid) return {}
  const targetGuid = stripGuidPrefix(raw.associatedMessageGuid)

  if (type === 'sticker' || type === '1000') {
    return { stickerFor: targetGuid }
  }

  const removed = type.startsWith('-')
  const name = removed ? type.slice(1) : type
  const named = REACTION_KIND[name]
  if (named) return { reaction: { targetGuid, kind: named, removed } }

  const numeric = Number(name)
  if (numeric >= 2000 && numeric < 3000) {
    const kind = NUMERIC_REACTION_KIND[numeric - 2000]
    return kind
      ? { reaction: { targetGuid, kind, removed: false } }
      : { reaction: { targetGuid, kind: 'emoji', emoji: extractEmoji(raw.text) ?? '❤️', removed: false } }
  }
  if (numeric >= 3000 && numeric < 4000) {
    const kind = NUMERIC_REACTION_KIND[numeric - 3000]
    return kind
      ? { reaction: { targetGuid, kind, removed: true } }
      : { reaction: { targetGuid, kind: 'emoji', emoji: extractEmoji(raw.text) ?? '❤️', removed: true } }
  }

  return {}
}

function parseGroupEvent(raw: RawMessage, who: Handle | undefined): GroupEvent | undefined {
  if (raw.itemType === 1) {
    return raw.groupActionType === 1 ? { kind: 'leave', who } : { kind: 'join', who }
  }
  if (raw.itemType === 2) {
    return { kind: 'rename', title: wellFormed(raw.groupTitle) ?? '' }
  }
  if (raw.itemType === 3) {
    return raw.groupActionType === 1 ? { kind: 'photo' } : { kind: 'leave', who }
  }
  return undefined
}

/** A single {UID: n} back-reference into the archive's flat $objects array. */
interface UIDRef {
  UID: number
}

function isUIDRef(value: unknown): value is UIDRef {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    typeof (value as Record<string, unknown>).UID === 'number' &&
    Object.keys(value as Record<string, unknown>).length === 1
  )
}

/**
 * Resolves an NSKeyedArchiver plist (already decoded from bplist to JSON by
 * the server) into a plain JS value: {UID: n} references are followed into
 * $objects, "$null" (index 0) becomes null, $class is dropped since it only
 * names the Obj-C class, NSURL/NSString wrappers collapse to their string,
 * and NSArray/NSDictionary wrappers collapse to a plain array/object.
 */
export function decodeKeyedArchive(archive: unknown): unknown {
  const root = archive as { $top?: { root?: unknown }; $objects?: unknown[] } | null | undefined
  const objects = root?.$objects ?? []
  const inProgress = new Set<number>()

  function walk(value: unknown): unknown {
    if (isUIDRef(value)) {
      const index = value.UID
      if (inProgress.has(index)) return undefined
      inProgress.add(index)
      const resolved = walk(objects[index])
      inProgress.delete(index)
      return resolved
    }
    if (value === '$null') return null
    if (Array.isArray(value)) return value.map(walk)
    if (value === null || typeof value !== 'object') return value

    const obj = value as Record<string, unknown>
    if ('NS.string' in obj) return walk(obj['NS.string'])
    if ('NS.relative' in obj) return walk(obj['NS.relative'])
    if ('NS.keys' in obj && 'NS.objects' in obj) {
      const keys = (obj['NS.keys'] as unknown[]).map(walk)
      const values = (obj['NS.objects'] as unknown[]).map(walk)
      const out: Record<string, unknown> = {}
      keys.forEach((key, i) => {
        if (typeof key === 'string') out[key] = values[i]
      })
      return out
    }
    if ('NS.objects' in obj) return (obj['NS.objects'] as unknown[]).map(walk)

    const out: Record<string, unknown> = {}
    for (const [key, val] of Object.entries(obj)) {
      if (key === '$class') continue
      out[key] = walk(val)
    }
    return out
  }

  return walk(root?.$top?.root)
}

interface RichLinkImage {
  richLinkImageAttachmentSubstituteIndex?: number
}

interface RichLinkMetadata {
  originalURL?: string
  URL?: string
  title?: string
  summary?: string
  siteName?: string
  image?: RichLinkImage
  icon?: RichLinkImage
}

/**
 * payloadData is an array with one element: the raw NSKeyedArchiver plist.
 * A rich-link's preview image (when it has one) isn't embedded in the
 * archive, it's one of the message's own attachments, referenced by index
 * via richLinkMetadata.image/icon.richLinkImageAttachmentSubstituteIndex.
 */
export function toUrlPreview(payloadData: unknown, attachments: RawAttachment[] = []): Message['urlPreview'] {
  const archive = Array.isArray(payloadData) ? payloadData[0] : payloadData
  if (!archive) return undefined

  let decoded: unknown
  try {
    decoded = decodeKeyedArchive(archive)
  } catch {
    return undefined
  }

  const metadata = (decoded as { richLinkMetadata?: RichLinkMetadata } | null | undefined)?.richLinkMetadata
  const url = metadata?.originalURL ?? metadata?.URL
  if (!metadata || !url) return undefined

  const substituteIndex = metadata.image?.richLinkImageAttachmentSubstituteIndex ?? metadata.icon?.richLinkImageAttachmentSubstituteIndex
  const imageAttachmentGuid = typeof substituteIndex === 'number' ? attachments[substituteIndex]?.guid : undefined

  return {
    url,
    title: wellFormed(metadata.title),
    summary: wellFormed(metadata.summary),
    siteName: wellFormed(metadata.siteName),
    imageAttachmentGuid,
  }
}

/**
 * One element of MessageResponse.attributedBody: the flat string Messages
 * stores plus the attribute runs laid over it. `range` is [start, length] in
 * UTF-16 code units, and an attachment's run covers exactly the one U+FFFC
 * placeholder that stands in for it, so run order gives the layout.
 */
interface AttributedRun {
  range?: [number, number]
  attributes?: Record<string, unknown>
}

interface AttributedBody {
  string?: string
  runs?: AttributedRun[]
}

const TEXT_EFFECT: Record<number, TextEffect> = {
  4: 'ripple',
  5: 'big',
  6: 'bloom',
  8: 'nod',
  9: 'shake',
  10: 'jitter',
  11: 'small',
  12: 'explode',
}

/** The formatting attributes all arrive as the integer 1; older bodies use a string. */
function isOn(value: unknown): boolean {
  return value === 1 || value === true || value === '1'
}

/** NSURL survives the server's archive decode as a plain string, but not always. */
function toLink(value: unknown): string | undefined {
  if (typeof value === 'string') return value || undefined
  if (value && typeof value === 'object') {
    const relative = (value as Record<string, unknown>)['NS.relative']
    if (typeof relative === 'string') return relative || undefined
  }
  return undefined
}

function styleOf(attributes: Record<string, unknown>): Omit<RichRun, 'text'> {
  const style: Omit<RichRun, 'text'> = {}
  if (isOn(attributes.__kIMTextBoldAttributeName)) style.bold = true
  if (isOn(attributes.__kIMTextItalicAttributeName)) style.italic = true
  if (isOn(attributes.__kIMTextUnderlineAttributeName)) style.underline = true
  if (isOn(attributes.__kIMTextStrikethroughAttributeName)) style.strike = true
  const link = toLink(attributes.__kIMLinkAttributeName)
  if (link) style.link = link
  const mention = attributes.__kIMMentionConfirmedMention
  if (typeof mention === 'string' && mention) style.mention = mention
  const effect = attributes.__kIMTextEffectAttributeName
  if (typeof effect === 'number') {
    const named = TEXT_EFFECT[effect]
    if (named) style.effect = named
  }
  return style
}

function isPlain(run: RichRun): boolean {
  return !run.bold && !run.italic && !run.underline && !run.strike && !run.link && !run.mention && !run.effect
}

function sameStyle(a: RichRun, b: RichRun): boolean {
  return (
    a.bold === b.bold &&
    a.italic === b.italic &&
    a.underline === b.underline &&
    a.strike === b.strike &&
    a.link === b.link &&
    a.mention === b.mention &&
    a.effect === b.effect
  )
}

function mergeRuns(runs: RichRun[]): RichRun[] {
  const out: RichRun[] = []
  for (const run of runs) {
    const previous = out[out.length - 1]
    if (previous && sameStyle(previous, run)) previous.text += run.text
    else out.push({ ...run })
  }
  // Trim the part's outer whitespace the way cleanText trims `text`, so the
  // newline Messages leaves around an attachment does not open a blank line.
  const first = out[0]
  if (first) first.text = first.text.replace(/^\s+/, '')
  const last = out[out.length - 1]
  if (last) last.text = last.text.replace(/\s+$/, '')
  return out.filter(run => run.text.length > 0)
}

/**
 * Splits an attributed body into the parts Messages laid out: text runs with
 * their formatting, and the attachments wherever their placeholder sits.
 * Returns undefined for a body that says nothing `text` does not already say,
 * so a plain message costs nothing downstream.
 */
export function toParts(attributedBody: unknown, attachments: RawAttachment[] = []): MessagePart[] | undefined {
  const body = (Array.isArray(attributedBody) ? attributedBody[0] : attributedBody) as AttributedBody | null | undefined
  const source = typeof body?.string === 'string' ? wellFormed(body.string) : undefined
  const runs = Array.isArray(body?.runs) ? body.runs : undefined
  if (source === undefined || !runs?.length) return undefined

  const known = new Set(attachments.map(a => a.guid))
  const parts: MessagePart[] = []
  let pending: RichRun[] = []
  let pendingPart: number | undefined

  const flush = () => {
    if (pending.length === 0) return
    const merged = mergeRuns(pending)
    if (merged.length) parts.push({ kind: 'text', runs: merged })
    pending = []
    pendingPart = undefined
  }

  for (const run of runs) {
    const attributes = (run?.attributes ?? {}) as Record<string, unknown>
    const start = run?.range?.[0]
    const length = run?.range?.[1]
    if (typeof start !== 'number' || typeof length !== 'number') continue

    const transfer = attributes.__kIMFileTransferGUIDAttributeName
    if (typeof transfer === 'string') {
      // An attachment the query did not ask for cannot be rendered, so drop
      // the placeholder rather than leave a U+FFFC glyph in the text.
      if (!known.has(transfer)) continue
      flush()
      parts.push({ kind: 'attachment', guid: transfer })
      continue
    }

    const text = source.slice(start, start + length).replace(/\uFFFC/g, '')
    if (!text) continue

    const partIndex = attributes.__kIMMessagePartAttributeName
    if (typeof partIndex === 'number') {
      if (pendingPart !== undefined && partIndex !== pendingPart) flush()
      pendingPart = partIndex
    }
    pending.push({ text, ...styleOf(attributes) })
  }
  flush()

  if (parts.length === 0) return undefined
  const only = parts.length === 1 ? parts[0] : undefined
  if (only?.kind === 'text' && only.runs.every(isPlain)) return undefined
  return parts
}

const CONVERTIBLE_IMAGE_EXT = new Set(['.heic', '.heif', '.tif', '.tiff'])

/**
 * GET /attachment/:guid/download only converts two source formats (sips for
 * HEIC/HEIF/TIFF -> JPEG, and CAF -> AAC/M4A) unless original=true is passed;
 * everything else comes back byte-identical either way. Request the
 * conversion for those two so the client gets a format it can actually
 * render/play, and request the original for everything else.
 */
export function downloadPlan(name?: string, mime?: string): { original: boolean; extension: string } {
  const ext = name ? extname(name).toLowerCase() : ''
  const isHeicHeifTiff = CONVERTIBLE_IMAGE_EXT.has(ext)
  const isCafAudio = mime === 'audio/x-caf' || ext === '.caf'
  const isImage = mime?.startsWith('image/') ?? false

  if (isHeicHeifTiff) return { original: false, extension: '.jpg' }
  if (isCafAudio) return { original: false, extension: '.m4a' }
  return { original: !isImage, extension: ext }
}

export function toHandle(raw: RawHandle, contacts?: ContactIndex): Handle {
  return {
    address: wellFormed(raw.address),
    service: toService(raw.service),
    name: contacts?.resolve(raw.address),
  }
}

export function toAttachment(raw: RawAttachment, localPath?: string): Attachment {
  return {
    guid: raw.guid,
    name: wellFormed(raw.transferName),
    mime: raw.mimeType,
    bytes: raw.totalBytes,
    width: raw.width,
    height: raw.height,
    isSticker: raw.isSticker ?? false,
    localPath,
    hidden: raw.hideAttachment ?? false,
    durationMs: typeof raw.metadata?.duration === 'number' ? raw.metadata.duration * 1000 : undefined,
  }
}

export function toMessage(raw: RawMessage, chatGuid?: string, options: MapOptions = {}): Message {
  const resolvedChatGuid = raw.chats?.[0]?.guid ?? chatGuid ?? ''
  const sender = raw.handle ? toHandle(raw.handle, options.contacts) : undefined
  // A message carries its own service via the sender's handle; a message
  // sent by me has no handle, so fall back to what the transport knows about
  // the chat, then the guid prefix, then default to iMessage.
  const service = sender?.service ?? options.chatService ?? prefixService(resolvedChatGuid) ?? 'iMessage'
  const { reaction, stickerFor } = parseAssociatedMessage(raw)
  const subject = raw.subject ? cleanText(raw.subject) : ''
  const urlPreview =
    raw.balloonBundleId === 'com.apple.messages.URLBalloonProvider'
      ? toUrlPreview(raw.payloadData, raw.attachments)
      : undefined

  return {
    guid: raw.guid,
    tempGuid: raw.tempGuid,
    chatGuid: resolvedChatGuid,
    text: cleanText(raw.text ?? ''),
    subject: subject || undefined,
    fromMe: raw.isFromMe,
    sender,
    date: raw.dateCreated,
    dateDelivered: raw.dateDelivered ?? undefined,
    dateRead: raw.dateRead ?? undefined,
    dateEdited: raw.dateEdited ?? undefined,
    dateRetracted: raw.dateRetracted ?? undefined,
    service,
    attachments: (raw.attachments ?? []).map(a => toAttachment(a, options.attachmentPaths?.get(a.guid))),
    tapbacks: [],
    parts: toParts(raw.attributedBody, raw.attachments),
    replyTo: raw.threadOriginatorGuid ? stripGuidPrefix(raw.threadOriginatorGuid) : undefined,
    stickerFor,
    effect: raw.expressiveSendStyleId ?? undefined,
    error: raw.error ? `Not delivered (error ${raw.error})` : undefined,
    isAudio: raw.isAudioMessage ?? false,
    groupEvent: raw.itemType !== 0 ? parseGroupEvent(raw, sender) : undefined,
    reaction,
    balloonBundleId: raw.balloonBundleId ?? undefined,
    urlPreview,
  }
}

export function toChat(raw: RawChat, options: MapOptions = {}): Chat {
  const fromParticipants = chatService(raw.guid, raw.participants)
  // On macOS 26 the chat row and its handles no longer say which service a
  // conversation uses; the latest message's handle does (for sent DMs it is
  // the recipient's handle).
  const service = raw.lastMessage?.handle?.service ? toService(raw.lastMessage.handle.service) : fromParticipants
  const lastMessage = raw.lastMessage ? toMessage(raw.lastMessage, raw.guid, { ...options, chatService: service }) : undefined
  return {
    guid: raw.guid,
    identifier: raw.chatIdentifier,
    service,
    isGroup: raw.style === 43,
    displayName: wellFormed(raw.displayName) || undefined,
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

  let name = wellFormed(raw.displayName)
  if (!name) {
    const firstName = wellFormed(raw.firstName)
    const lastName = wellFormed(raw.lastName)
    if (firstName && lastName) name = `${firstName} ${lastName}`
    else if (firstName) name = firstName
    else if (raw.nickname) name = wellFormed(raw.nickname)
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
