import type { Chat, Contact, FocusStatus, Handle, Message, ScheduledMessage, ServerInfo, Service, TapbackKind } from './model'

export type ConnectionStatus = 'connecting' | 'online' | 'offline'

/**
 * A definite answer from the server (a 4xx or 5xx envelope). Anything else
 * that a request throws, a dropped socket or a timeout, is worth retrying.
 */
export class TransportError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message)
    this.name = 'TransportError'
  }
}

export function isRetryable(error: unknown): boolean {
  return !(error instanceof TransportError)
}

export type TransportEvent =
  | { type: 'connection'; status: ConnectionStatus; error?: string }
  | { type: 'server'; info: ServerInfo }
  /** A new or updated message. Reactions arrive here too, with `reaction` set. */
  | { type: 'message'; message: Message }
  | { type: 'chat'; chat: Chat }
  | { type: 'chat-removed'; chatGuid: string }
  /** The address book, once the transport has fetched it. */
  | { type: 'contacts'; contacts: Contact[] }
  | { type: 'typing'; chatGuid: string; typing: boolean }
  | { type: 'read'; chatGuid: string; read: boolean }
  /** A ringing or ended FaceTime call on the Mac. `canAnswer` is false on the legacy event path, which only names the caller. */
  | { type: 'facetime'; callUuid: string; status: 'incoming' | 'ended'; from?: Handle; canAnswer: boolean }

export interface Page<T> {
  items: T[]
  hasMore: boolean
}

export interface SendTextOptions {
  replyTo?: string
  effect?: string
  subject?: string
  tempGuid?: string
}

export interface SendAttachmentOptions {
  name?: string
  isAudio?: boolean
  tempGuid?: string
}

export interface SearchFilters {
  chatGuid?: string
  /** From an `in:` filter. An empty array (no chat matched) returns nothing, unlike an absent one. */
  chatGuids?: string[]
  limit?: number
  /** Epoch ms lower bound. The store's reconcile sweep also uses this, with no `before`, to list everything created since the last pass. */
  after?: number
  /** Epoch ms upper bound, inclusive. */
  before?: number
  fromMe?: boolean
  /** Addresses, already resolved from a `from:` filter's name or address. */
  senders?: string[]
  attachments?: 'image' | 'video' | 'file'
  links?: boolean
}

export interface Transport {
  readonly kind: 'bluebubbles' | 'demo'
  /** Resolves once the server answered and the event stream is open. Rejects on the first failure; the store retries. */
  connect(): Promise<ServerInfo>
  disconnect(): void
  subscribe(listener: (event: TransportEvent) => void): () => void
  /** Last known address book, so names resolve before the server's own list arrives. */
  seedContacts(contacts: Contact[]): void

  listChats(options?: { limit?: number; offset?: number }): Promise<Page<Chat>>
  getChat(chatGuid: string): Promise<Chat>
  loadMessages(chatGuid: string, options: { limit: number; before?: number }): Promise<Page<Message>>
  /** Empty `query` with `after` lists everything created since that time; the store uses it to reconcile. */
  searchMessages(query: string, options?: SearchFilters): Promise<Message[]>
  listContacts(): Promise<Contact[]>

  sendText(chatGuid: string, text: string, options?: SendTextOptions): Promise<Message>
  sendAttachment(chatGuid: string, path: string, options?: SendAttachmentOptions): Promise<Message>
  /** Downloads into the attachment cache when needed and returns the local path. */
  attachmentPath(attachmentGuid: string, options?: { name?: string; mime?: string }): Promise<string>

  createChat(addresses: string[], firstMessage: string, service?: Service): Promise<Chat>
  markRead(chatGuid: string): Promise<void>
  deleteChat(chatGuid: string): Promise<void>

  /** The server holds the message and sends it once `sendAt` passes; the client does no waiting of its own. */
  scheduleText(chatGuid: string, text: string, sendAt: number): Promise<ScheduledMessage>
  listScheduled(): Promise<ScheduledMessage[]>
  cancelScheduled(id: string): Promise<void>

  // Everything below needs the private API (SIP disabled, helper connected).
  react(chatGuid: string, messageGuid: string, kind: TapbackKind, options?: { emoji?: string; remove?: boolean; partIndex?: number }): Promise<void>
  setTyping(chatGuid: string, typing: boolean): Promise<void>
  markUnread(chatGuid: string): Promise<void>
  editMessage(chatGuid: string, messageGuid: string, text: string, options?: { partIndex?: number; backwardsCompatText?: string }): Promise<Message>
  unsendMessage(chatGuid: string, messageGuid: string, options?: { partIndex?: number }): Promise<void>
  renameGroup(chatGuid: string, name: string): Promise<void>
  addParticipant(chatGuid: string, address: string): Promise<void>
  removeParticipant(chatGuid: string, address: string): Promise<void>
  leaveGroup(chatGuid: string): Promise<void>
  setGroupIcon(chatGuid: string, path: string): Promise<void>
  /** Whether a Focus is silencing the person at `address`. They only share it with people they have allowed to. */
  focusStatus(address: string): Promise<FocusStatus>
  /** Breaks a Focus for one message I sent: the "Notify Anyway" button. */
  notifySilenced(chatGuid: string, messageGuid: string): Promise<void>
  /** Creates a FaceTime Link on the Mac and returns it. */
  createFaceTimeLink(): Promise<string>
  /** Answers a ringing call on the Mac and returns a FaceTime Link that joins it from a browser. */
  answerFaceTime(callUuid: string): Promise<string>
  leaveFaceTime(callUuid: string): Promise<void>
}
