import { createHash } from 'node:crypto'
import { mkdir, stat } from 'node:fs/promises'
import { basename, join } from 'node:path'
import { io, type Socket } from 'socket.io-client'
import type { Chat, Contact, Handle, Message, ServerInfo, Service, TapbackKind } from '../model'
import type {
  Page,
  SendAttachmentOptions,
  SendTextOptions,
  Transport,
  TransportEvent,
} from '../transport'
import {
  ContactIndex,
  downloadPlan,
  toChat,
  toContact,
  toHandle,
  toMessage,
  toServerInfo,
  type RawChat,
  type RawContact,
  type RawHandle,
  type RawMessage,
  type RawServerInfo,
} from './map'

export interface BlueBubblesOptions {
  url: string
  password: string
  attachmentsDir: string
}

export class BlueBubblesError extends Error {
  constructor(public status: number, message: string) {
    super(message)
    this.name = 'BlueBubblesError'
  }
}

interface Envelope<T> {
  status: number
  message: string
  data?: T
  metadata?: unknown
  error?: { type: string; message: string }
  encrypted?: boolean
}

/**
 * packages/server/src/server/api/lib/facetime/FaceTimeSession.ts FaceTimeSessionStatus
 * and packages/server/src/server/api/privateApi/eventHandlers/PrivateApiFaceTimeStatusHandler.ts
 */
interface RawFaceTimeStatus {
  uuid: string
  status_id: number
  status: string
  address: string
  handle?: RawHandle | null
  is_outgoing: boolean
}

const MESSAGE_EVENTS = ['new-message', 'updated-message', 'message-send-error']
const GROUP_EVENTS = [
  'group-name-change',
  'participant-added',
  'participant-removed',
  'participant-left',
  'group-icon-changed',
  'group-icon-removed',
]

type Query = Record<string, string | number | boolean | undefined>

export class BlueBubblesTransport implements Transport {
  readonly kind = 'bluebubbles' as const

  private socket: Socket | undefined
  private contacts = new ContactIndex()
  private serverInfo: ServerInfo | null = null
  private readonly listeners = new Set<(event: TransportEvent) => void>()
  private readonly downloads = new Map<string, Promise<string>>()
  // any; chat guids on macOS 26 carry no service of their own; toMessage()
  // needs the chat's service for messages sent by me (no handle to read it
  // from), so mapChat() fills this in every time a chat is mapped.
  private readonly chatServices = new Map<string, Service>()
  private avatarsReady: Promise<void> | null = null
  // Group icons that answered 404 this session, so a chat with no photo is
  // not re-requested on every reconcile pass.
  private readonly iconMisses = new Set<string>()
  private iconSlotsInUse = 0
  private readonly iconWaiters: Array<() => void> = []

  constructor(private readonly options: BlueBubblesOptions) {}

  async connect(): Promise<ServerInfo> {
    this.emit({ type: 'connection', status: 'connecting' })
    try {
      const info = await this.fetchServerInfo()
      this.serverInfo = info
      this.emit({ type: 'server', info })

      try {
        this.contacts = new ContactIndex(await this.listContacts())
      } catch (err) {
        console.error('BlueBubbles: failed to load contacts', err)
      }

      await this.openSocket()
      return info
    } catch (err) {
      this.emit({ type: 'connection', status: 'offline', error: errorMessage(err) })
      throw err
    }
  }

  disconnect(): void {
    this.socket?.disconnect()
    this.socket = undefined
  }

  subscribe(listener: (event: TransportEvent) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  async listChats(options: { limit?: number; offset?: number } = {}): Promise<Page<Chat>> {
    const limit = options.limit ?? 200
    const offset = options.offset ?? 0
    const raw = await this.request<RawChat[]>('POST', '/chat/query', {
      // participants come back by default; "lastmessage" also flips the sort.
      json: { with: ['participants', 'lastmessage'], sort: 'lastmessage', limit, offset },
    })
    const items = await Promise.all(raw.map(chat => this.mapChat(chat)))
    return { items, hasMore: raw.length === limit }
  }

  async getChat(chatGuid: string): Promise<Chat> {
    const raw = await this.request<RawChat>('GET', `/chat/${encodeURIComponent(chatGuid)}`, {
      query: { with: 'participants' },
    })
    return this.mapChat(raw)
  }

  async loadMessages(chatGuid: string, options: { limit: number; before?: number }): Promise<Page<Message>> {
    const raw = await this.request<RawMessage[]>('GET', `/chat/${encodeURIComponent(chatGuid)}/message`, {
      // on this route the bare "payloadData" is ignored; it has to be "message.payloadData".
      // attributedBody is only matched under the prefixed name on some builds, so send both.
      query: {
        with: 'attachments,message.payloadData,message.attributedBody,attributedBody',
        sort: 'DESC',
        limit: options.limit,
        before: options.before,
      },
    })
    const attachmentPaths = await this.resolveAttachmentPaths(raw)
    const chatService = this.chatServices.get(chatGuid)
    const items = raw
      .map(m => toMessage(m, chatGuid, { contacts: this.contacts, attachmentPaths, chatService }))
      .sort((a, b) => a.date - b.date)
    return { items, hasMore: raw.length === options.limit }
  }

  async searchMessages(
    query: string,
    options: { chatGuid?: string; limit?: number; after?: number } = {},
  ): Promise<Message[]> {
    // Empty query + after is the store's reconciliation sweep: list everything
    // created since that time, oldest first, with no text filter at all.
    const sweep = query === '' && options.after != null
    const json: Record<string, unknown> = {
      with: ['chats', 'attachments', 'payloadData', 'attributedBody'],
      sort: sweep ? 'ASC' : 'DESC',
      limit: options.limit ?? 50,
      chatGuid: options.chatGuid,
      after: options.after,
    }
    if (query) {
      json.where = [{ statement: 'message.text LIKE :text COLLATE NOCASE', args: { text: `%${query}%` } }]
    }

    const raw = await this.request<RawMessage[]>('POST', '/message/query', { json })
    const attachmentPaths = await this.resolveAttachmentPaths(raw)
    return raw.map(m => {
      const resolvedChatGuid = m.chats?.[0]?.guid ?? options.chatGuid
      return toMessage(m, options.chatGuid, {
        contacts: this.contacts,
        attachmentPaths,
        chatService: resolvedChatGuid ? this.chatServices.get(resolvedChatGuid) : undefined,
      })
    })
  }

  async listContacts(): Promise<Contact[]> {
    const raw = await this.fetchRawContacts()
    return Promise.all(raw.map(async item => toContact(item, await this.saveContactAvatar(item))))
  }

  // extraProperties=avatar is a separate opt-in on top of the plain contact
  // list; fall back to the unadorned call if a server build rejects it.
  private async fetchRawContacts(): Promise<RawContact[]> {
    try {
      return await this.request<RawContact[]>('GET', '/contact', { query: { extraProperties: 'avatar' } })
    } catch {
      return this.request<RawContact[]>('GET', '/contact')
    }
  }

  async sendText(chatGuid: string, text: string, options: SendTextOptions = {}): Promise<Message> {
    const json: Record<string, unknown> = {
      chatGuid,
      message: text,
      method: this.sendMethod(),
      tempGuid: options.tempGuid,
      effectId: options.effect,
      subject: options.subject,
    }
    if (options.replyTo) {
      json.selectedMessageGuid = options.replyTo
      json.partIndex = 0
    }
    const raw = await this.request<RawMessage>('POST', '/message/text', { json })
    return this.mapMessage(raw, chatGuid)
  }

  async sendAttachment(chatGuid: string, path: string, options: SendAttachmentOptions = {}): Promise<Message> {
    const name = options.name ?? basename(path)
    const form = new FormData()
    form.append('attachment', Bun.file(path), name)
    form.append('chatGuid', chatGuid)
    form.append('method', this.sendMethod())
    form.append('name', name)
    if (options.tempGuid) form.append('tempGuid', options.tempGuid)
    if (options.isAudio) form.append('isAudioMessage', 'true')

    const raw = await this.request<RawMessage>('POST', '/message/attachment', { form })
    return this.mapMessage(raw, chatGuid)
  }

  async attachmentPath(attachmentGuid: string, options: { name?: string; mime?: string } = {}): Promise<string> {
    const { original, extension } = downloadPlan(options.name, options.mime)
    const path = this.attachmentCachePath(attachmentGuid, extension)
    if (await Bun.file(path).exists()) return path

    const pending = this.downloads.get(attachmentGuid)
    if (pending) return pending

    const download = (async () => {
      const response = await this.download(`/attachment/${encodeURIComponent(attachmentGuid)}/download`, {
        original,
      })
      await Bun.write(path, await response.arrayBuffer())
      return path
    })()

    this.downloads.set(attachmentGuid, download)
    try {
      return await download
    } finally {
      this.downloads.delete(attachmentGuid)
    }
  }

  async createChat(addresses: string[], firstMessage: string, service?: Service): Promise<Chat> {
    const raw = await this.request<RawChat>('POST', '/chat/new', {
      json: { addresses, message: firstMessage, service },
    })
    return this.mapChat(raw)
  }

  async markRead(chatGuid: string): Promise<void> {
    await this.request('POST', `/chat/${encodeURIComponent(chatGuid)}/read`)
  }

  async deleteChat(chatGuid: string): Promise<void> {
    await this.request('DELETE', `/chat/${encodeURIComponent(chatGuid)}`)
  }

  async react(
    chatGuid: string,
    messageGuid: string,
    kind: TapbackKind,
    options: { emoji?: string; remove?: boolean; partIndex?: number } = {},
  ): Promise<void> {
    if (kind === 'emoji') {
      // packages/server/src/server/api/interfaces/messageInterface.ts possibleReactions
      // is fixed to the six named tapbacks (plus their "-" removals); there is
      // no custom-emoji reaction and no associatedMessageEmoji field to carry one.
      throw new BlueBubblesError(400, 'BlueBubbles does not support custom emoji tapbacks')
    }
    const reaction = options.remove ? `-${kind}` : kind
    await this.request('POST', '/message/react', {
      json: { chatGuid, selectedMessageGuid: messageGuid, reaction, partIndex: options.partIndex },
    })
  }

  async setTyping(chatGuid: string, typing: boolean): Promise<void> {
    await this.request(typing ? 'POST' : 'DELETE', `/chat/${encodeURIComponent(chatGuid)}/typing`)
  }

  async markUnread(chatGuid: string): Promise<void> {
    await this.request('POST', `/chat/${encodeURIComponent(chatGuid)}/unread`)
  }

  async editMessage(
    chatGuid: string,
    messageGuid: string,
    text: string,
    options: { partIndex?: number; backwardsCompatText?: string } = {},
  ): Promise<Message> {
    const raw = await this.request<RawMessage>('POST', `/message/${encodeURIComponent(messageGuid)}/edit`, {
      json: {
        editedMessage: text,
        backwardsCompatibilityMessage: options.backwardsCompatText ?? text,
        partIndex: options.partIndex,
      },
    })
    return this.mapMessage(raw, chatGuid)
  }

  async unsendMessage(chatGuid: string, messageGuid: string, options: { partIndex?: number } = {}): Promise<void> {
    await this.request('POST', `/message/${encodeURIComponent(messageGuid)}/unsend`, {
      json: { partIndex: options.partIndex },
    })
  }

  async renameGroup(chatGuid: string, name: string): Promise<void> {
    await this.request('PUT', `/chat/${encodeURIComponent(chatGuid)}`, { json: { displayName: name } })
  }

  async addParticipant(chatGuid: string, address: string): Promise<void> {
    await this.request('POST', `/chat/${encodeURIComponent(chatGuid)}/participant/add`, { json: { address } })
  }

  async removeParticipant(chatGuid: string, address: string): Promise<void> {
    await this.request('POST', `/chat/${encodeURIComponent(chatGuid)}/participant/remove`, { json: { address } })
  }

  async leaveGroup(chatGuid: string): Promise<void> {
    await this.request('POST', `/chat/${encodeURIComponent(chatGuid)}/leave`)
  }

  async setGroupIcon(chatGuid: string, path: string): Promise<void> {
    const form = new FormData()
    form.append('icon', Bun.file(path), basename(path))
    await this.request('POST', `/chat/${encodeURIComponent(chatGuid)}/icon`, { form })
  }

  async notifySilenced(chatGuid: string, messageGuid: string): Promise<void> {
    await this.request('POST', `/message/${encodeURIComponent(messageGuid)}/notify`)
  }

  async createFaceTimeLink(): Promise<string> {
    const data = await this.request<{ link: string }>('POST', '/facetime/session')
    return data.link
  }

  // The server answers on the Mac, generates a link for the call, admits the
  // first joiner and hangs up its own side about 15 seconds later
  // (bluebubbles-helper#38). The link is still what a browser can join.
  async answerFaceTime(callUuid: string): Promise<string> {
    const data = await this.request<{ link: string }>('POST', `/facetime/answer/${encodeURIComponent(callUuid)}`)
    return data.link
  }

  async leaveFaceTime(callUuid: string): Promise<void> {
    await this.request('POST', `/facetime/leave/${encodeURIComponent(callUuid)}`)
  }

  private sendMethod(): 'private-api' | 'apple-script' {
    return this.serverInfo?.privateApi && this.serverInfo.helperConnected ? 'private-api' : 'apple-script'
  }

  private async mapMessage(raw: RawMessage, chatGuid?: string): Promise<Message> {
    const attachmentPaths = await this.resolveAttachmentPaths([raw])
    const resolvedChatGuid = raw.chats?.[0]?.guid ?? chatGuid
    return toMessage(raw, chatGuid, {
      contacts: this.contacts,
      attachmentPaths,
      chatService: resolvedChatGuid ? this.chatServices.get(resolvedChatGuid) : undefined,
    })
  }

  private async mapChat(raw: RawChat): Promise<Chat> {
    const attachmentPaths = raw.lastMessage ? await this.resolveAttachmentPaths([raw.lastMessage]) : undefined
    const chatIcon = raw.style === 43 ? await this.fetchChatIcon(raw.guid) : undefined
    const chat = toChat(raw, { contacts: this.contacts, attachmentPaths, chatIcon })
    this.chatServices.set(chat.guid, chat.service)
    return chat
  }

  private attachmentCachePath(guid: string, extension: string): string {
    return `${this.options.attachmentsDir}/${guid}${extension}`
  }

  private avatarsDir(): string {
    return join(this.options.attachmentsDir, '..', 'avatars')
  }

  private ensureAvatarsDir(): Promise<void> {
    this.avatarsReady ??= mkdir(this.avatarsDir(), { recursive: true }).then(() => undefined)
    return this.avatarsReady
  }

  private async saveContactAvatar(raw: RawContact): Promise<string | undefined> {
    if (!raw.avatar) return undefined
    const bytes = Buffer.from(raw.avatar, 'base64')
    const path = join(this.avatarsDir(), `contact-${raw.id}.jpg`)
    const existing = await stat(path).catch(() => undefined)
    if (existing?.size === bytes.byteLength) return path
    await this.ensureAvatarsDir()
    await Bun.write(path, bytes)
    return path
  }

  private chatIconPath(chatGuid: string): string {
    const hash = createHash('sha1').update(chatGuid).digest('hex')
    return join(this.avatarsDir(), `chat-${hash}.jpg`)
  }

  /**
   * GET /chat/:guid/icon 404s for a group with no photo. Misses are
   * remembered for the session, and a fetch that has not answered in 3s is
   * abandoned, so a slow or absent icon never holds up the chat list.
   */
  private async fetchChatIcon(chatGuid: string): Promise<string | undefined> {
    if (this.iconMisses.has(chatGuid)) return undefined
    const path = this.chatIconPath(chatGuid)
    if (await Bun.file(path).exists()) return path

    return this.withIconSlot(async () => {
      if (await Bun.file(path).exists()) return path
      try {
        const response = await fetch(this.buildUrl(`/chat/${encodeURIComponent(chatGuid)}/icon`), {
          signal: AbortSignal.timeout(3_000),
        })
        if (response.status === 404) {
          this.iconMisses.add(chatGuid)
          return undefined
        }
        if (!response.ok) return undefined
        await this.ensureAvatarsDir()
        await Bun.write(path, await response.arrayBuffer())
        return path
      } catch {
        return undefined
      }
    })
  }

  private async withIconSlot<T>(run: () => Promise<T>): Promise<T> {
    if (this.iconSlotsInUse >= 6) {
      await new Promise<void>(resolve => this.iconWaiters.push(resolve))
    }
    this.iconSlotsInUse += 1
    try {
      return await run()
    } finally {
      this.iconSlotsInUse -= 1
      this.iconWaiters.shift()?.()
    }
  }

  private async resolveAttachmentPaths(messages: RawMessage[]): Promise<Map<string, string>> {
    const paths = new Map<string, string>()
    await Promise.all(
      messages.flatMap(message =>
        (message.attachments ?? []).map(async attachment => {
          const { extension } = downloadPlan(attachment.transferName, attachment.mimeType)
          const path = this.attachmentCachePath(attachment.guid, extension)
          if (await Bun.file(path).exists()) paths.set(attachment.guid, path)
        }),
      ),
    )
    return paths
  }

  private openSocket(): Promise<void> {
    return new Promise((resolve, reject) => {
      const socket = io(this.options.url, {
        query: { password: this.options.password },
        transports: ['websocket'],
      })
      this.socket = socket
      let settled = false

      socket.on('connect', () => {
        this.emit({ type: 'connection', status: 'online' })
        if (!settled) {
          settled = true
          resolve()
        }
        this.fetchServerInfo()
          .then(info => {
            this.serverInfo = info
            this.emit({ type: 'server', info })
          })
          .catch(err => console.error('BlueBubbles: failed to refresh server info', err))
      })

      socket.on('connect_error', (err: Error) => {
        this.emit({ type: 'connection', status: 'offline', error: err.message })
        if (!settled) {
          settled = true
          reject(err)
        }
      })

      socket.on('disconnect', (reason: string) => {
        this.emit({ type: 'connection', status: 'offline', error: reason })
      })

      this.bindEvents(socket)
    })
  }

  private bindEvents(socket: Socket): void {
    for (const event of MESSAGE_EVENTS) {
      socket.on(event, (raw: RawMessage) => {
        this.mapMessage(raw)
          .then(message => this.emit({ type: 'message', message }))
          .catch(err => console.error(`BlueBubbles: failed to handle "${event}"`, err))
      })
    }

    for (const event of GROUP_EVENTS) {
      socket.on(event, (raw: RawMessage) => {
        this.mapMessage(raw)
          .then(message => {
            this.emit({ type: 'message', message })
            return this.getChat(message.chatGuid)
          })
          .then(chat => this.emit({ type: 'chat', chat }))
          .catch(err => console.error(`BlueBubbles: failed to handle "${event}"`, err))
      })
    }

    socket.on('chat-read-status-changed', (data: { chatGuid: string; read: boolean }) => {
      this.emit({ type: 'read', chatGuid: data.chatGuid, read: data.read })
    })

    socket.on('typing-indicator', (data: { display: boolean; guid: string }) => {
      this.emit({ type: 'typing', chatGuid: data.guid, typing: data.display })
    })

    socket.on('ft-call-status-changed', (data: RawFaceTimeStatus) => {
      // "incoming" is a ringing call, "disconnected" ends one; outgoing,
      // answered and unknown are not worth an event.
      if (data.status !== 'incoming' && data.status !== 'disconnected') return
      this.emit({
        type: 'facetime',
        callUuid: data.uuid,
        status: data.status === 'incoming' ? 'incoming' : 'ended',
        from: data.handle ? toHandle(data.handle, this.contacts) : undefined,
        canAnswer: data.status === 'incoming',
      })
    })

    // Legacy path when "FaceTime Calling" is off in the server settings: a JSON
    // string naming the caller, with no call uuid to answer.
    socket.on('incoming-facetime', (raw: unknown) => {
      let caller: string | undefined
      try {
        const parsed = typeof raw === 'string' ? (JSON.parse(raw) as { caller?: string }) : (raw as { caller?: string })
        caller = parsed?.caller
      } catch {
        caller = undefined
      }
      this.emit({
        type: 'facetime',
        callUuid: '',
        status: 'incoming',
        from: caller ? toHandle({ originalROWID: 0, address: caller, service: 'iMessage' }, this.contacts) : undefined,
        canAnswer: false,
      })
    })
  }

  private emit(event: TransportEvent): void {
    for (const listener of this.listeners) listener(event)
  }

  private async fetchServerInfo(): Promise<ServerInfo> {
    const raw = await this.request<RawServerInfo>('GET', '/server/info')
    return toServerInfo(raw)
  }

  private buildUrl(path: string, query: Query = {}): string {
    const url = new URL(`/api/v1${path}`, this.options.url)
    url.searchParams.set('password', this.options.password)
    for (const [key, value] of Object.entries(query)) {
      if (value !== undefined) url.searchParams.set(key, String(value))
    }
    return url.toString()
  }

  private async request<T>(
    method: 'GET' | 'POST' | 'PUT' | 'DELETE',
    path: string,
    init: { query?: Query; json?: unknown; form?: FormData } = {},
  ): Promise<T> {
    const headers: Record<string, string> = {}
    let body: BodyInit | undefined
    if (init.form) {
      body = init.form
    } else if (init.json !== undefined) {
      headers['content-type'] = 'application/json'
      body = JSON.stringify(init.json)
    }

    const response = await fetch(this.buildUrl(path, init.query), {
      method,
      headers,
      body,
      signal: AbortSignal.timeout(30_000),
    })
    const envelope = (await response.json()) as Envelope<T>
    return this.unwrap(response, envelope)
  }

  private async download(path: string, query: Query = {}): Promise<Response> {
    const response = await fetch(this.buildUrl(path, query), { signal: AbortSignal.timeout(5 * 60_000) })
    if (!response.ok) {
      throw new BlueBubblesError(response.status, `Failed to download attachment (status ${response.status})`)
    }
    return response
  }

  private unwrap<T>(response: Response, envelope: Envelope<T>): T {
    if (envelope.encrypted) {
      // packages/server/src/server/api/http/api/v1/socketRoutes.ts encrypts the
      // payload with the server password when "Encrypt communications" is on.
      throw new BlueBubblesError(response.status, 'Turn off "Encrypt communications" in the BlueBubbles server settings')
    }
    if (!response.ok || envelope.status >= 400) {
      const message = envelope.error?.message ?? envelope.message ?? `Request failed (status ${response.status})`
      throw new BlueBubblesError(envelope.status ?? response.status, message)
    }
    return envelope.data as T
  }
}

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}
