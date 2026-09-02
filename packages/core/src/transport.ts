import type { Chat, Contact, Handle, Message, ServerInfo, Service, TapbackKind } from './model'

export type ConnectionStatus = 'connecting' | 'online' | 'offline'

export type TransportEvent =
  | { type: 'connection'; status: ConnectionStatus; error?: string }
  | { type: 'server'; info: ServerInfo }
  /** A new or updated message. Reactions arrive here too, with `reaction` set. */
  | { type: 'message'; message: Message }
  | { type: 'chat'; chat: Chat }
  | { type: 'chat-removed'; chatGuid: string }
  | { type: 'typing'; chatGuid: string; typing: boolean }
  | { type: 'read'; chatGuid: string; read: boolean }
  | { type: 'facetime'; callUuid: string; from?: Handle; link?: string }

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

export interface Transport {
  readonly kind: 'bluebubbles' | 'demo'
  connect(): Promise<ServerInfo>
  disconnect(): void
  subscribe(listener: (event: TransportEvent) => void): () => void

  listChats(options?: { limit?: number; offset?: number }): Promise<Page<Chat>>
  getChat(chatGuid: string): Promise<Chat>
  loadMessages(chatGuid: string, options: { limit: number; before?: number }): Promise<Page<Message>>
  /** Empty `query` with `after` lists everything created since that time; the store uses it to reconcile. */
  searchMessages(query: string, options?: { chatGuid?: string; limit?: number; after?: number }): Promise<Message[]>
  listContacts(): Promise<Contact[]>

  sendText(chatGuid: string, text: string, options?: SendTextOptions): Promise<Message>
  sendAttachment(chatGuid: string, path: string, options?: SendAttachmentOptions): Promise<Message>
  /** Downloads into the attachment cache when needed and returns the local path. */
  attachmentPath(attachmentGuid: string, options?: { name?: string }): Promise<string>

  createChat(addresses: string[], firstMessage: string, service?: Service): Promise<Chat>
  markRead(chatGuid: string): Promise<void>
  deleteChat(chatGuid: string): Promise<void>

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
  notifySilenced(chatGuid: string, messageGuid: string): Promise<void>
  startFaceTime(address: string): Promise<string | undefined>
}
