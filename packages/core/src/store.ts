import { MacAgentClient, isPinned, type AgentConfig, type ChatPrefs, type SharedPrefs } from './agent'
import { normalizeAddress, type FriendLocation } from './findmy'
import { mergeGifFavorites, type Gif, type GifFavorite } from './gifs'
import { openExternal } from './open'
import { imageSize } from './image'
import { snapshotForCache, type StateCache } from './cache'
import { writeConversationExport } from './export'
import { conversationGuid, conversationHandles, conversationHasOlder, conversationMembers, conversationMessages, groupChats, type Grouping } from './conversations'
import {
  capabilitiesFor,
  handleName,
  type Capabilities,
  type Chat,
  type Contact,
  type Message,
  type Reaction,
  type ScheduledMessage,
  type ServerInfo,
  type Service,
  type Tapback,
  type TapbackKind,
} from './model'
import { isRetryable, type ConnectionStatus, type Transport, type TransportEvent } from './transport'

export interface FaceTimeCall {
  callUuid: string
  from?: string
  status: 'incoming' | 'answering' | 'ready' | 'failed' | 'ended'
  canAnswer: boolean
  link?: string
  error?: string
}

export interface AppState extends Grouping {
  status: ConnectionStatus
  /** Why the last connection attempt failed, while `status` is not `online`. */
  connectionError?: string
  /** A failed action worth a toast. Connection trouble stays in `connectionError`. */
  error?: string
  server: ServerInfo | null
  capabilities: Capabilities
  chats: Chat[]
  /** Visible rows per chat, ascending by date. Reactions are folded into `tapbacks`. */
  messages: Record<string, Message[]>
  hasOlder: Record<string, boolean>
  loading: Record<string, boolean>
  typing: Record<string, boolean>
  selectedChat: string | null
  drafts: Record<string, string>
  contacts: Contact[]
  scheduled: ScheduledMessage[]
  /** Guid of the message being replied to in the composer, per chat. */
  replyingTo: Record<string, string | undefined>
  /** Guid of the message being edited in the composer, per chat. */
  editing: Record<string, string | undefined>
  facetime: FaceTimeCall | null
  lastSyncAt: number
  /** Find My friend locations, keyed by `normalizeAddress`. */
  locations: Record<string, FriendLocation>
  locationsUpdatedAt: number
  /** `off` when no Mac agent is configured; `unavailable` once a fetch has failed. */
  findMy: 'off' | 'unavailable' | 'ok'
  /** Guid of the conversation currently paging its history for an export, so the UI can say so. */
  exportingChat: string | null
  /** Favorited GIFs, keyed by gif id, synced through the Mac agent same as chat prefs. */
  gifFavorites: Record<string, GifFavorite>
}

export interface StoreOptions {
  prefs?: Record<string, ChatPrefs>
  onPrefsChange?: (prefs: Record<string, ChatPrefs>) => void
  gifFavorites?: Record<string, GifFavorite>
  onGifFavoritesChange?: (favorites: Record<string, GifFavorite>) => void
  onIncoming?: (chat: Chat, message: Message, target?: Message) => void
  pageSize?: number
  reconcileEveryMs?: number
  /** Address of the `@messages/mac-agent` on the Mac. Omit to leave Find My and prefs sync off. */
  agent?: AgentConfig
  /** Last known state, painted before the server answers and kept current afterwards. */
  cache?: StateCache
}

const LOCATIONS_POLL_MS = 60_000
const LOCATIONS_MIN_INTERVAL_MS = 20_000

const PAGE = 50
/** The server is slow per message once attributedBody is requested, and one big request can hang it for minutes. */
const SWEEP_PAGE = 10
const TYPING_IDLE_MS = 3000
/** Messages.app drops a typing bubble after about a minute if the other side never sends; so do we, in case the stop event is lost. */
const TYPING_SHOWN_MAX_MS = 60_000
const CONNECT_RETRY_MS = 2000
const CONNECT_RETRY_MAX_MS = 30_000
const SEND_ATTEMPTS = 4
const SEND_RETRY_MS = 2000
const DRAFT_SYNC_DEBOUNCE_MS = 2000
/** Exporting stops once the conversation runs out of history or hits this many messages. */
const EXPORT_MAX_MESSAGES = 2000

let tempCounter = 0
function nextTempGuid(): string {
  tempCounter += 1
  return `temp-${Date.now()}-${tempCounter}`
}

function sortChats(chats: Chat[]): Chat[] {
  return [...chats].sort((a, b) => {
    if (a.pinned !== b.pinned) return a.pinned ? -1 : 1
    return b.lastActivity - a.lastActivity
  })
}

function insertSorted(list: Message[], message: Message): Message[] {
  const next = list.slice()
  let index = next.length
  while (index > 0 && (next[index - 1]?.date ?? 0) > message.date) index -= 1
  next.splice(index, 0, message)
  return next
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

/** An optimistic row and a server row describe the same send when text and attachment names agree. */
function sameSend(mine: Message, theirs: Message): boolean {
  if (mine.text !== theirs.text) return false
  const names = (message: Message) => message.attachments.map((item) => item.name).join('\n')
  return names(mine) === names(theirs)
}

/** A send waiting its turn. Sends go out one at a time, in order, and wait for the connection to come back. */
interface Outgoing {
  chatGuid: string
  tempGuid: string
  optimistic: Message
  attempts: number
  run: () => Promise<Message>
}

export class MessagesStore {
  state: AppState
  private listeners = new Set<() => void>()
  private prefs: Record<string, ChatPrefs>
  private options: StoreOptions
  private unsubscribe: (() => void) | null = null
  private pendingReactions = new Map<string, Reaction[]>()
  private typingTimers = new Map<string, ReturnType<typeof setTimeout>>()
  private typingSent = new Set<string>()
  private typingShown = new Map<string, ReturnType<typeof setTimeout>>()
  private draftSyncTimers = new Map<string, ReturnType<typeof setTimeout>>()
  /** When the composer for a chat was last typed into, so a stale remote draft never overwrites newer local text. */
  private draftEditedAt = new Map<string, number>()
  private reconcileTimer: ReturnType<typeof setInterval> | null = null
  private reconciling = false
  private agent: MacAgentClient | null
  private locationsTimer: ReturnType<typeof setInterval> | null = null
  private lastLocationsFetchAt = 0
  private outbox: Outgoing[] = []
  private flushing = false
  private stopped = false
  private wakers = new Set<() => void>()

  constructor(
    public readonly transport: Transport,
    options: StoreOptions = {},
  ) {
    this.options = options
    this.prefs = options.prefs ?? {}
    this.agent = options.agent ? new MacAgentClient(options.agent) : null
    this.state = {
      status: 'connecting',
      server: null,
      capabilities: capabilitiesFor(null),
      chats: [],
      messages: {},
      hasOlder: {},
      loading: {},
      typing: {},
      selectedChat: null,
      drafts: {},
      contacts: [],
      scheduled: [],
      replyingTo: {},
      editing: {},
      facetime: null,
      lastSyncAt: Date.now(),
      locations: {},
      locationsUpdatedAt: 0,
      findMy: 'off',
      exportingChat: null,
      gifFavorites: options.gifFavorites ?? {},
      primaryOf: {},
      merged: {},
    }
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  getSnapshot = (): AppState => this.state

  private set(patch: Partial<AppState>): void {
    let next = { ...this.state, ...patch }
    if ('chats' in patch || 'contacts' in patch) {
      const grouping = groupChats(next.chats, next.contacts)
      // The open thread follows its conversation when a newer member becomes the primary.
      const selected = next.selectedChat ? conversationGuid(grouping, next.selectedChat) : null
      next = { ...next, ...grouping, selectedChat: selected }
    }
    this.state = next
    for (const listener of this.listeners) listener()
    if (this.options.cache && ('chats' in patch || 'messages' in patch || 'contacts' in patch || 'selectedChat' in patch)) {
      this.options.cache.schedule(() => snapshotForCache(this.state))
    }
  }

  async start(): Promise<void> {
    const cached = this.options.cache ? await this.options.cache.load() : null
    if (cached && cached.chats.length > 0) {
      const hasOlder: Record<string, boolean> = {}
      for (const guid of Object.keys(cached.messages)) hasOlder[guid] = true
      this.set({
        chats: sortChats(cached.chats.map((chat) => this.withPrefs(chat))),
        messages: cached.messages,
        contacts: cached.contacts,
        hasOlder,
        selectedChat: cached.selectedChat && cached.chats.some((chat) => chat.guid === cached.selectedChat) ? cached.selectedChat : (cached.chats[0]?.guid ?? null),
        lastSyncAt: cached.savedAt,
      })
      this.transport.seedContacts(cached.contacts)
    }
    this.unsubscribe = this.transport.subscribe((event) => this.handle(event))
    const info = await this.connectUntilUp()
    if (!info) return
    if (cached && this.state.chats.length > 0) {
      // Painted from disk already; bring the list and the open thread up to date.
      this.set({ status: 'online' })
      await this.reconcile()
      if (this.state.selectedChat && this.state.hasOlder[this.state.selectedChat] === undefined) await this.loadOlder(this.state.selectedChat)
    } else {
      await this.refreshChats()
      const first = this.state.chats[0]
      if (first && !this.state.selectedChat) await this.selectChat(first.guid)
    }
    if (this.state.capabilities.scheduledMessages) await this.refreshScheduled()
    const every = this.options.reconcileEveryMs ?? 30_000
    if (every > 0) this.reconcileTimer = setInterval(() => void this.reconcile(), every)
    void this.syncPrefs()
    void this.refreshLocations()
  }

  stop(): void {
    this.stopped = true
    for (const wake of this.wakers) wake()
    this.unsubscribe?.()
    this.unsubscribe = null
    if (this.reconcileTimer) clearInterval(this.reconcileTimer)
    this.reconcileTimer = null
    if (this.locationsTimer) clearInterval(this.locationsTimer)
    this.locationsTimer = null
    for (const timer of this.typingTimers.values()) clearTimeout(timer)
    for (const timer of this.typingShown.values()) clearTimeout(timer)
    for (const timer of this.draftSyncTimers.values()) clearTimeout(timer)
    this.transport.disconnect()
    void this.options.cache?.flush()
  }

  /** Keeps trying with backoff until the server answers. Only `stop()` gives up. */
  private async connectUntilUp(): Promise<ServerInfo | null> {
    let delay = CONNECT_RETRY_MS
    while (!this.stopped) {
      try {
        const info = await this.transport.connect()
        this.set({ server: info, capabilities: capabilitiesFor(info), connectionError: undefined })
        return info
      } catch (error) {
        this.set({ status: 'offline', connectionError: errorText(error) })
        await this.pause(delay)
        delay = Math.min(delay * 2, CONNECT_RETRY_MAX_MS)
      }
    }
    return null
  }

  /** A sleep that `stop()` and a reconnect can cut short. */
  private pause(ms: number): Promise<void> {
    return new Promise((resolve) => {
      const wake = () => {
        clearTimeout(timer)
        this.wakers.delete(wake)
        resolve()
      }
      const timer = setTimeout(wake, ms)
      this.wakers.add(wake)
    })
  }

  /** The details panel is the only Find My consumer, so it drives the poll: on while it's open, off otherwise. */
  setDetailsOpen(open: boolean): void {
    if (open) {
      if (this.locationsTimer) return
      void this.refreshLocations()
      this.locationsTimer = setInterval(() => void this.refreshLocations(), LOCATIONS_POLL_MS)
    } else {
      if (this.locationsTimer) clearInterval(this.locationsTimer)
      this.locationsTimer = null
    }
  }

  async refreshLocations(): Promise<void> {
    if (!this.agent) return
    const now = Date.now()
    if (now - this.lastLocationsFetchAt < LOCATIONS_MIN_INTERVAL_MS) return
    this.lastLocationsFetchAt = now
    try {
      const { friends } = await this.agent.friends()
      const locations: Record<string, FriendLocation> = {}
      for (const friend of friends) for (const address of friend.addresses) locations[normalizeAddress(address)] = friend
      this.set({ locations, locationsUpdatedAt: Date.now(), findMy: 'ok' })
    } catch (error) {
      // Quiet: a Mac with no agent running, or one that's asleep, is a normal
      // state, not an error worth surfacing through `state.error`.
      console.error(`findmy: ${String(error)}`)
      this.set({ findMy: 'unavailable' })
    }
  }

  private handle(event: TransportEvent): void {
    switch (event.type) {
      case 'connection': {
        const wasOffline = this.state.status !== 'online'
        this.set({ status: event.status, connectionError: event.status === 'online' ? undefined : event.error })
        if (event.status === 'online' && wasOffline) {
          for (const wake of this.wakers) wake()
          if (this.state.chats.length > 0) void this.reconcile()
          void this.flushOutbox()
          void this.syncPrefs()
        }
        return
      }
      case 'server':
        this.set({ server: event.info, capabilities: capabilitiesFor(event.info) })
        return
      case 'contacts':
        this.set({ contacts: event.contacts })
        return
      case 'message':
        this.applyMessage(event.message, { fromServer: true })
        return
      case 'chat':
        this.upsertChat(event.chat)
        return
      case 'chat-removed':
        this.set({ chats: this.state.chats.filter((chat) => chat.guid !== event.chatGuid) })
        return
      case 'typing':
        this.showTyping(this.resolveChatGuid(event.chatGuid), event.typing)
        return
      case 'read':
        this.patchChat(this.resolveChatGuid(event.chatGuid), { unread: !event.read })
        return
      case 'facetime': {
        const from = event.from ? handleName(event.from) : undefined
        if (event.status === 'ended') {
          const current = this.state.facetime
          if (current && current.callUuid === event.callUuid && current.status !== 'ready') this.set({ facetime: { ...current, status: 'ended' } })
          return
        }
        this.set({ facetime: { callUuid: event.callUuid, from, status: 'incoming', canAnswer: event.canAnswer } })
        return
      }
    }
  }

  // Safety net for anything the socket dropped: the chat list and the open
  // thread are re-read, and messages created since the last pass are folded
  // in, one small page at a time so a long absence catches up progressively.
  async reconcile(): Promise<void> {
    if (this.state.status !== 'online' || this.reconciling) return
    this.reconciling = true
    const startedAt = Date.now()
    try {
      await this.refreshChats()
      let since = this.state.lastSyncAt
      for (;;) {
        const recent = await this.transport.searchMessages('', { limit: SWEEP_PAGE, after: since })
        for (const message of recent) this.applyMessage(message, { fromServer: true, silent: true })
        if (recent.length < SWEEP_PAGE) break
        // `after` is exclusive, so the newest date of the page is the next cursor.
        since = Math.max(since + 1, ...recent.map((message) => message.date))
        this.set({ lastSyncAt: since })
        if (this.stopped || this.state.status !== 'online') return
      }
      this.set({ lastSyncAt: startedAt })
      const selected = this.state.selectedChat
      if (selected) {
        const page = await this.transport.loadMessages(selected, { limit: PAGE })
        for (const message of page.items) this.applyMessage(message, { fromServer: true, silent: true })
      }
      if (this.state.capabilities.scheduledMessages) await this.refreshScheduled()
    } catch (error) {
      console.error(`reconcile: ${String(error)}`)
    } finally {
      this.reconciling = false
    }
  }

  async refreshScheduled(): Promise<void> {
    try {
      const scheduled = await this.transport.listScheduled()
      this.set({ scheduled })
    } catch (error) {
      console.error(`scheduled: ${String(error)}`)
    }
  }

  async refreshChats(): Promise<void> {
    const pageSize = 200
    const chats: Chat[] = []
    for (let offset = 0; offset < 5000; offset += pageSize) {
      const page = await this.transport.listChats({ limit: pageSize, offset })
      chats.push(...page.items.map((chat) => this.withPrefs(chat)))
      // On first load the first page is enough to paint. A refresh keeps the
      // full list on screen until the new one is complete.
      if (offset === 0 && this.state.chats.length === 0) this.set({ chats: sortChats(chats) })
      if (!page.hasMore) break
    }
    for (const chat of chats) {
      if (chat.lastMessage) this.stashReaction(chat.lastMessage)
    }
    const known = new Map(chats.map((chat) => [chat.guid, chat]))
    // Chats that arrived through the socket since the pass started stay.
    for (const chat of this.state.chats) if (!known.has(chat.guid)) known.set(chat.guid, chat)
    this.set({ chats: sortChats([...known.values()]) })
  }

  /** Private API events may name a chat with its old `iMessage;-;` prefix while chat.db on macOS 26 says `any;-;`. Match on the identifier. */
  private resolveChatGuid(guid: string): string {
    if (this.state.chats.some((chat) => chat.guid === guid)) return guid
    const separator = guid.indexOf(';')
    const identifier = separator >= 0 ? guid.slice(guid.indexOf(';', separator + 1) + 1) : guid
    const match = this.state.chats.find((chat) => chat.identifier === identifier || chat.guid.endsWith(`;${identifier}`))
    return match?.guid ?? guid
  }

  /** The chat behind an entry of the Mac's pin list: chat.db's group id for a group, an address for a one-to-one chat. */
  private chatForPin(identifier: string): Chat | undefined {
    const found = this.chatMatching(identifier)
    if (!found) return undefined
    const primary = conversationGuid(this.state, found.guid)
    return primary === found.guid ? found : this.state.chats.find((chat) => chat.guid === primary)
  }

  private chatMatching(identifier: string): Chat | undefined {
    const byId = this.state.chats.find((chat) => chat.groupId === identifier || chat.guid === identifier)
    if (byId) return byId
    if (identifier.includes(';')) {
      const guid = this.resolveChatGuid(identifier)
      return this.state.chats.find((chat) => chat.guid === guid)
    }
    const wanted = normalizeAddress(identifier)
    return this.state.chats.find((chat) => !chat.isGroup && chat.participants.some((handle) => normalizeAddress(handle.address) === wanted))
  }

  private withPrefs(chat: Chat): Chat {
    const prefs = this.prefs[chat.guid]
    return { ...chat, pinned: isPinned(prefs), muted: prefs?.muted ?? false, readReceipts: prefs?.readReceipts ?? true }
  }

  private upsertChat(chat: Chat): void {
    const next = this.withPrefs(chat)
    const exists = this.state.chats.some((item) => item.guid === chat.guid)
    const chats = exists ? this.state.chats.map((item) => (item.guid === chat.guid ? { ...item, ...next } : item)) : [...this.state.chats, next]
    this.set({ chats: sortChats(chats) })
  }

  private patchChat(chatGuid: string, patch: Partial<Chat>): void {
    if (!this.state.chats.some((chat) => chat.guid === chatGuid)) return
    this.set({ chats: sortChats(this.state.chats.map((chat) => (chat.guid === chatGuid ? { ...chat, ...patch } : chat))) })
  }

  async selectChat(guid: string | null): Promise<void> {
    const chatGuid = guid ? conversationGuid(this.state, guid) : null
    const previous = this.state.selectedChat
    if (previous && previous !== chatGuid) void this.stopTyping(previous)
    this.set({ selectedChat: chatGuid })
    if (!chatGuid) return
    // A chat the socket or the sweep touched holds a few recent rows and no page boundary yet; page it before showing it.
    await Promise.all(conversationMembers(this.state, chatGuid).map((member) => (this.state.hasOlder[member] === undefined ? this.loadOlder(member) : undefined)))
    const unread = conversationMembers(this.state, chatGuid).some((member) => this.state.chats.find((item) => item.guid === member)?.unread)
    if (unread) void this.markRead(chatGuid)
  }

  /** Pages every member of the conversation back by one page. */
  async loadEarlier(guid: string): Promise<void> {
    await Promise.all(conversationMembers(this.state, guid).map((member) => this.loadOlder(member)))
  }

  async loadOlder(chatGuid: string): Promise<void> {
    if (this.state.loading[chatGuid]) return
    if (this.state.messages[chatGuid] && this.state.hasOlder[chatGuid] === false) return
    this.set({ loading: { ...this.state.loading, [chatGuid]: true } })
    try {
      const oldest = this.state.messages[chatGuid]?.[0]?.date
      const page = await this.transport.loadMessages(chatGuid, { limit: PAGE, before: oldest })
      for (const message of page.items) this.applyMessage(message, { fromServer: true, silent: true })
      this.set({
        messages: { ...this.state.messages, [chatGuid]: this.state.messages[chatGuid] ?? [] },
        hasOlder: { ...this.state.hasOlder, [chatGuid]: page.hasMore },
      })
    } catch (error) {
      this.set({ error: errorText(error) })
    } finally {
      this.set({ loading: { ...this.state.loading, [chatGuid]: false } })
    }
  }

  private stashReaction(message: Message, options: { fromServer?: boolean; silent?: boolean } = {}): boolean {
    if (!message.reaction) return false
    const target = this.findMessage(message.chatGuid, message.reaction.targetGuid)
    if (options.fromServer && !options.silent && !message.fromMe && !message.reaction.removed) {
      const chat = this.state.chats.find((item) => item.guid === message.chatGuid)
      if (chat && !chat.muted) this.options.onIncoming?.(chat, message, target)
    }
    if (target) {
      this.applyReactionTo(message.chatGuid, target, message)
    } else {
      const pending = this.pendingReactions.get(message.reaction.targetGuid) ?? []
      this.pendingReactions.set(message.reaction.targetGuid, [...pending, { ...message.reaction, guid: message.guid, fromMe: message.fromMe, sender: message.sender } as Reaction])
    }
    return true
  }

  /** Looks through every member of the conversation, so a guid found in the merged thread resolves. */
  private findMessage(chatGuid: string, guid: string): Message | undefined {
    for (const member of conversationMembers(this.state, chatGuid)) {
      const found = this.state.messages[member]?.find((message) => message.guid === guid)
      if (found) return found
    }
    return undefined
  }

  private applyReactionTo(chatGuid: string, target: Message, reaction: Message): void {
    const detail = reaction.reaction
    if (!detail) return
    const tapback: Tapback = { guid: reaction.guid, kind: detail.kind, emoji: detail.emoji, fromMe: reaction.fromMe, sender: reaction.sender }
    const sameAuthor = (item: Tapback) => (item.fromMe && tapback.fromMe) || (!item.fromMe && !tapback.fromMe && item.sender?.address === tapback.sender?.address)
    let tapbacks = target.tapbacks.filter((item) => !(sameAuthor(item) && item.kind === tapback.kind && item.emoji === tapback.emoji))
    if (!detail.removed) tapbacks = [...tapbacks.filter((item) => !sameAuthor(item)), tapback]
    this.replaceMessage(chatGuid, { ...target, tapbacks })
  }

  private replaceMessage(_chatGuid: string, message: Message): void {
    const chatGuid = message.chatGuid
    const list = this.state.messages[chatGuid] ?? []
    this.set({ messages: { ...this.state.messages, [chatGuid]: list.map((item) => (item.guid === message.guid ? message : item)) } })
  }

  applyMessage(incoming: Message, options: { fromServer?: boolean; silent?: boolean } = {}): void {
    const chatGuid = incoming.chatGuid
    if (incoming.reaction) {
      this.stashReaction(incoming, options)
      return
    }
    const list = this.state.messages[chatGuid] ?? []
    let existingIndex = list.findIndex(
      (item) => item.guid === incoming.guid || (incoming.tempGuid && (item.guid === incoming.tempGuid || item.tempGuid === incoming.tempGuid)),
    )
    // The socket echoes a send before its HTTP reply lands, and for an
    // attachment the echo carries no temp guid, so match it to the optimistic
    // row by content instead of letting it sit beside it.
    if (existingIndex < 0 && options.fromServer && incoming.fromMe && !incoming.tempGuid) {
      existingIndex = list.findIndex((item) => item.tempGuid && item.guid === item.tempGuid && !item.error && sameSend(item, incoming))
    }
    let message = incoming
    let next: Message[]
    if (existingIndex >= 0) {
      const existing = list[existingIndex]!
      message = { ...existing, ...incoming, tapbacks: incoming.tapbacks.length ? incoming.tapbacks : existing.tapbacks, tempGuid: existing.tempGuid ?? incoming.tempGuid }
      next = list.slice()
      next.splice(existingIndex, 1)
      next = insertSorted(next, message)
    } else {
      const pending = this.pendingReactions.get(incoming.guid)
      if (pending) {
        this.pendingReactions.delete(incoming.guid)
        message = { ...incoming, tapbacks: this.foldPending(incoming.tapbacks, pending) }
      }
      next = insertSorted(list, message)
    }
    const isNew = existingIndex < 0
    this.set({ messages: { ...this.state.messages, [chatGuid]: next } })

    const chat = this.state.chats.find((item) => item.guid === chatGuid)
    const newest = next[next.length - 1]
    if (chat && newest && newest.guid === message.guid) {
      const selectedAndVisible = this.state.selectedChat === conversationGuid(this.state, chatGuid)
      const unread = isNew && !message.fromMe && !selectedAndVisible ? true : selectedAndVisible ? false : chat.unread
      this.patchChat(chatGuid, { lastMessage: message, lastActivity: Math.max(chat.lastActivity, message.date), unread })
      if (isNew && !message.fromMe && options.fromServer && !options.silent && !chat.muted) this.options.onIncoming?.(chat, message)
      if (isNew && !message.fromMe && selectedAndVisible && this.state.capabilities.readReceipts) void this.markRead(chatGuid)
    } else if (!chat && options.fromServer) {
      void this.transport
        .getChat(chatGuid)
        .then((fetched) => this.upsertChat(fetched))
        .catch(() => undefined)
    }
    if (isNew && !message.fromMe) this.showTyping(chatGuid, false)
  }

  private foldPending(existing: Tapback[], pending: Reaction[]): Tapback[] {
    let tapbacks = existing.slice()
    for (const item of pending) {
      const entry = item as Reaction & { guid: string; fromMe: boolean; sender?: Message['sender'] }
      const same = (t: Tapback) => (t.fromMe && entry.fromMe) || (!t.fromMe && !entry.fromMe && t.sender?.address === entry.sender?.address)
      tapbacks = tapbacks.filter((t) => !same(t))
      if (!entry.removed) tapbacks.push({ guid: entry.guid, kind: entry.kind, emoji: entry.emoji, fromMe: entry.fromMe, sender: entry.sender })
    }
    return tapbacks
  }

  setDraft(chatGuid: string, text: string): void {
    this.set({ drafts: { ...this.state.drafts, [chatGuid]: text } })
    this.scheduleDraftSync(chatGuid, text)
    if (!this.state.capabilities.typing) return
    if (text.length === 0) {
      void this.stopTyping(chatGuid)
      return
    }
    if (!this.typingSent.has(chatGuid)) {
      this.typingSent.add(chatGuid)
      void this.transport.setTyping(chatGuid, true).catch(() => this.typingSent.delete(chatGuid))
    }
    const existing = this.typingTimers.get(chatGuid)
    if (existing) clearTimeout(existing)
    this.typingTimers.set(chatGuid, setTimeout(() => void this.stopTyping(chatGuid), TYPING_IDLE_MS))
  }

  /** Debounces the draft into `prefs` and out to the agent, off for the demo transport. */
  private scheduleDraftSync(chatGuid: string, text: string): void {
    if (this.transport.kind === 'demo') return
    this.draftEditedAt.set(chatGuid, Date.now())
    const existing = this.draftSyncTimers.get(chatGuid)
    if (existing) clearTimeout(existing)
    this.draftSyncTimers.set(
      chatGuid,
      setTimeout(() => {
        this.draftSyncTimers.delete(chatGuid)
        this.savePrefs(chatGuid, { draft: text })
      }, DRAFT_SYNC_DEBOUNCE_MS),
    )
  }

  /** Cancels a pending draft sync and, if the agent might still hold one, tells it the draft is gone. */
  private clearDraftSync(chatGuid: string): void {
    const timer = this.draftSyncTimers.get(chatGuid)
    if (timer) clearTimeout(timer)
    this.draftSyncTimers.delete(chatGuid)
    this.draftEditedAt.delete(chatGuid)
    if (this.transport.kind === 'demo' || !this.prefs[chatGuid]?.draft) return
    this.savePrefs(chatGuid, { draft: '' })
  }

  private showTyping(chatGuid: string, typing: boolean): void {
    const timer = this.typingShown.get(chatGuid)
    if (timer) clearTimeout(timer)
    this.typingShown.delete(chatGuid)
    if (typing) this.typingShown.set(chatGuid, setTimeout(() => this.showTyping(chatGuid, false), TYPING_SHOWN_MAX_MS))
    if (Boolean(this.state.typing[chatGuid]) === typing) return
    this.set({ typing: { ...this.state.typing, [chatGuid]: typing } })
  }

  private async stopTyping(chatGuid: string): Promise<void> {
    const timer = this.typingTimers.get(chatGuid)
    if (timer) clearTimeout(timer)
    this.typingTimers.delete(chatGuid)
    if (!this.typingSent.delete(chatGuid)) return
    await this.transport.setTyping(chatGuid, false).catch(() => undefined)
  }

  setReplyingTo(chatGuid: string, messageGuid: string | undefined): void {
    this.set({ replyingTo: { ...this.state.replyingTo, [chatGuid]: messageGuid }, editing: { ...this.state.editing, [chatGuid]: undefined } })
  }

  setEditing(chatGuid: string, messageGuid: string | undefined): void {
    const message = messageGuid ? this.findMessage(chatGuid, messageGuid) : undefined
    this.set({
      editing: { ...this.state.editing, [chatGuid]: messageGuid },
      replyingTo: { ...this.state.replyingTo, [chatGuid]: undefined },
      drafts: { ...this.state.drafts, [chatGuid]: message?.text ?? '' },
    })
  }

  async send(chatGuid: string, text: string, options: { effect?: string } = {}): Promise<void> {
    const body = text.trim()
    if (!body) return
    const editingGuid = this.state.editing[chatGuid]
    this.set({ drafts: { ...this.state.drafts, [chatGuid]: '' }, editing: { ...this.state.editing, [chatGuid]: undefined } })
    this.clearDraftSync(chatGuid)
    void this.stopTyping(chatGuid)
    if (editingGuid) {
      await this.edit(chatGuid, editingGuid, body)
      return
    }
    const replyTo = this.state.replyingTo[chatGuid]
    this.set({ replyingTo: { ...this.state.replyingTo, [chatGuid]: undefined } })
    const tempGuid = nextTempGuid()
    const optimistic: Message = {
      guid: tempGuid,
      tempGuid,
      chatGuid,
      text: body,
      fromMe: true,
      date: Date.now(),
      service: this.serviceFor(chatGuid),
      attachments: [],
      tapbacks: [],
      replyTo,
      effect: options.effect,
      isAudio: false,
    }
    this.applyMessage(optimistic)
    this.enqueue({ chatGuid, tempGuid, optimistic, attempts: 0, run: () => this.transport.sendText(chatGuid, body, { replyTo, effect: options.effect, tempGuid }) })
  }

  async scheduleSend(chatGuid: string, text: string, sendAt: number): Promise<void> {
    const body = text.trim()
    if (!body) return
    this.set({
      drafts: { ...this.state.drafts, [chatGuid]: '' },
      editing: { ...this.state.editing, [chatGuid]: undefined },
      replyingTo: { ...this.state.replyingTo, [chatGuid]: undefined },
    })
    void this.stopTyping(chatGuid)
    try {
      const message = await this.transport.scheduleText(chatGuid, body, sendAt)
      this.set({ scheduled: [...this.state.scheduled, message] })
    } catch (error) {
      this.set({ error: error instanceof Error ? error.message : String(error) })
    }
  }

  async cancelScheduled(id: string): Promise<void> {
    const previous = this.state.scheduled
    this.set({ scheduled: previous.filter((item) => item.id !== id) })
    try {
      await this.transport.cancelScheduled(id)
    } catch (error) {
      this.set({ scheduled: previous, error: error instanceof Error ? error.message : String(error) })
    }
  }

  async sendAttachment(chatGuid: string, path: string): Promise<void> {
    const tempGuid = nextTempGuid()
    const name = path.split('/').pop() ?? 'attachment'
    const file = Bun.file(path)
    const optimistic: Message = {
      guid: tempGuid,
      tempGuid,
      chatGuid,
      text: '',
      fromMe: true,
      date: Date.now(),
      service: this.serviceFor(chatGuid),
      attachments: [{ guid: tempGuid, name, mime: file.type || 'application/octet-stream', bytes: file.size, isSticker: false, hidden: false, localPath: path }],
      tapbacks: [],
      isAudio: false,
    }
    this.applyMessage(optimistic)
    this.enqueue({
      chatGuid,
      tempGuid,
      optimistic,
      attempts: 0,
      run: async () => {
        const sent = await this.transport.sendAttachment(chatGuid, path, { name, tempGuid })
        return { ...sent, attachments: sent.attachments.map((item) => ({ ...item, localPath: item.localPath ?? path })) }
      },
    })
  }

  private enqueue(item: Outgoing): void {
    this.outbox.push(item)
    void this.flushOutbox()
  }

  /** How many sends are still waiting for the server. */
  get pendingSends(): number {
    return this.outbox.length
  }

  // One send at a time, in order. A send the server refused fails right away;
  // one the network lost is retried, and the whole queue waits out a dropped
  // connection instead of failing every message behind it.
  private async flushOutbox(): Promise<void> {
    if (this.flushing) return
    this.flushing = true
    try {
      while (this.outbox.length > 0 && !this.stopped) {
        if (this.state.status !== 'online') return
        const item = this.outbox[0]!
        // A reply lost on the way back is not a lost send: once the socket echo has replaced the optimistic row, the send is done.
        if (this.settled(item)) {
          this.outbox.shift()
          continue
        }
        try {
          const sent = await item.run()
          this.outbox.shift()
          this.applyMessage({ ...sent, tempGuid: item.tempGuid })
        } catch (error) {
          if (this.settled(item)) {
            this.outbox.shift()
            continue
          }
          item.attempts += 1
          if (!isRetryable(error) || item.attempts >= SEND_ATTEMPTS) {
            this.outbox.shift()
            this.applyMessage({ ...item.optimistic, error: errorText(error) })
          } else {
            await this.pause(SEND_RETRY_MS * item.attempts)
          }
        }
      }
    } finally {
      this.flushing = false
    }
  }

  private settled(item: Outgoing): boolean {
    const row = (this.state.messages[item.chatGuid] ?? []).find((message) => message.tempGuid === item.tempGuid)
    return Boolean(row && row.guid !== row.tempGuid)
  }

  /** The service the conversation is actually on: the latest message wins over the chat row, which macOS 26 no longer types. */
  private serviceFor(chatGuid: string): Service {
    const list = this.state.messages[chatGuid] ?? []
    for (let index = list.length - 1; index >= 0; index -= 1) {
      const message = list[index]!
      if (!message.groupEvent && !message.reaction) return message.service
    }
    return this.state.chats.find((item) => item.guid === chatGuid)?.service ?? 'iMessage'
  }

  async retry(chatGuid: string, messageGuid: string): Promise<void> {
    const failed = this.findMessage(chatGuid, messageGuid)
    if (!failed?.error) return
    this.set({ messages: { ...this.state.messages, [failed.chatGuid]: (this.state.messages[failed.chatGuid] ?? []).filter((item) => item.guid !== messageGuid) } })
    const attachment = failed.attachments[0]
    if (attachment?.localPath) await this.sendAttachment(chatGuid, attachment.localPath)
    else await this.send(chatGuid, failed.text, { effect: failed.effect })
  }

  async react(chatGuid: string, messageGuid: string, kind: TapbackKind, emoji?: string): Promise<void> {
    const target = this.findMessage(chatGuid, messageGuid)
    if (!target) return
    const mine = target.tapbacks.find((item) => item.fromMe)
    const remove = mine?.kind === kind && mine.emoji === emoji
    const optimisticGuid = `temp-tapback-${Date.now()}`
    const tapbacks = target.tapbacks.filter((item) => !item.fromMe)
    if (!remove) tapbacks.push({ guid: optimisticGuid, kind, emoji, fromMe: true })
    this.replaceMessage(chatGuid, { ...target, tapbacks })
    try {
      await this.transport.react(target.chatGuid, messageGuid, kind, { emoji, remove })
    } catch (error) {
      this.replaceMessage(chatGuid, target)
      this.set({ error: errorText(error) })
    }
  }

  async edit(chatGuid: string, messageGuid: string, text: string): Promise<void> {
    const target = this.findMessage(chatGuid, messageGuid)
    if (!target) return
    this.replaceMessage(chatGuid, { ...target, text, dateEdited: Date.now() })
    try {
      const updated = await this.transport.editMessage(target.chatGuid, messageGuid, text, { backwardsCompatText: `Edited to “${text}”` })
      this.applyMessage(updated)
    } catch (error) {
      this.replaceMessage(chatGuid, target)
      this.set({ error: errorText(error) })
    }
  }

  async unsend(chatGuid: string, messageGuid: string): Promise<void> {
    const target = this.findMessage(chatGuid, messageGuid)
    if (!target) return
    this.replaceMessage(chatGuid, { ...target, dateRetracted: Date.now() })
    try {
      await this.transport.unsendMessage(target.chatGuid, messageGuid)
    } catch (error) {
      this.replaceMessage(chatGuid, target)
      this.set({ error: errorText(error) })
    }
  }

  async markRead(chatGuid: string): Promise<void> {
    // "Read without receipts" clears the dot for every member but skips the network call, so the other side never learns.
    const sendReceipt = this.prefs[conversationGuid(this.state, chatGuid)]?.readReceipts ?? true
    for (const member of conversationMembers(this.state, chatGuid)) {
      if (!this.state.chats.find((chat) => chat.guid === member)?.unread) continue
      this.patchChat(member, { unread: false })
      if (sendReceipt && this.state.capabilities.readReceipts) await this.transport.markRead(member).catch(() => undefined)
    }
  }

  async markUnread(chatGuid: string): Promise<void> {
    this.patchChat(chatGuid, { unread: true })
    if (!this.state.capabilities.markUnread) return
    await this.transport.markUnread(chatGuid).catch(() => undefined)
  }

  togglePin(chatGuid: string): void {
    const chat = this.state.chats.find((item) => item.guid === chatGuid)
    if (!chat) return
    this.savePrefs(chatGuid, { pinned: !chat.pinned })
    this.patchChat(chatGuid, { pinned: !chat.pinned })
  }

  toggleMute(chatGuid: string): void {
    const chat = this.state.chats.find((item) => item.guid === chatGuid)
    if (!chat) return
    this.savePrefs(chatGuid, { muted: !chat.muted })
    this.patchChat(chatGuid, { muted: !chat.muted })
  }

  toggleReadReceipts(chatGuid: string): void {
    const chat = this.state.chats.find((item) => item.guid === chatGuid)
    if (!chat) return
    const readReceipts = !(chat.readReceipts ?? true)
    this.savePrefs(chatGuid, { readReceipts })
    this.patchChat(chatGuid, { readReceipts })
  }

  private savePrefs(chatGuid: string, patch: ChatPrefs): void {
    this.prefs = { ...this.prefs, [chatGuid]: { ...this.prefs[chatGuid], ...patch, updatedAt: Date.now() } }
    this.options.onPrefsChange?.(this.prefs)
    void this.syncPrefs()
  }

  /** A removal keeps a tombstone (`removed: true`) with a fresh `updatedAt`, so it wins over an older favorite synced from another client. */
  toggleGifFavorite(gif: Gif): void {
    const wasFavorite = Boolean(this.state.gifFavorites[gif.id] && !this.state.gifFavorites[gif.id]?.removed)
    const entry: GifFavorite = wasFavorite ? { gif, updatedAt: Date.now(), removed: true } : { gif, updatedAt: Date.now() }
    const gifFavorites = { ...this.state.gifFavorites, [gif.id]: entry }
    this.set({ gifFavorites })
    this.options.onGifFavoritesChange?.(gifFavorites)
    void this.syncPrefs()
  }

  /**
   * Pins and mutes travel through the Mac agent: this client's entries go up,
   * the merged set and the pins made in Messages.app on the Mac come back.
   */
  async syncPrefs(): Promise<void> {
    if (!this.agent || this.state.status !== 'online') return
    const mine: Record<string, ChatPrefs> = {}
    for (const [guid, entry] of Object.entries(this.prefs)) {
      if (entry.updatedAt) mine[guid] = { pinned: entry.pinned, muted: entry.muted, readReceipts: entry.readReceipts, draft: entry.draft, updatedAt: entry.updatedAt }
    }
    try {
      this.applySharedPrefs(await this.agent.syncPrefs(mine, this.state.gifFavorites))
    } catch (error) {
      console.error(`prefs: ${String(error)}`)
    }
  }

  private applySharedPrefs(shared: SharedPrefs): void {
    const next: Record<string, ChatPrefs> = {}
    // Pins made before sync existed carry no time. Once the Mac's own list is in, that list is the truth and they yield to it.
    const macListKnown = shared.macPinnedAt !== null
    for (const [guid, entry] of Object.entries(this.prefs)) {
      next[guid] = { ...entry, macPinned: undefined, macPinnedAt: undefined }
      if (macListKnown && entry.pinned && !entry.updatedAt) delete next[guid]!.pinned
    }
    for (const [guid, entry] of Object.entries(shared.chats)) {
      const local = next[guid]
      if (!local?.updatedAt || (entry.updatedAt ?? 0) > local.updatedAt) {
        next[guid] = { ...local, pinned: entry.pinned, muted: entry.muted, readReceipts: entry.readReceipts, draft: entry.draft, updatedAt: entry.updatedAt }
      }
    }
    for (const identifier of shared.macPinned) {
      const chat = this.chatForPin(identifier)
      if (chat) next[chat.guid] = { ...next[chat.guid], macPinned: true, macPinnedAt: shared.macPinnedAt ?? 0 }
    }
    // The draft that won the merge above still yields to text typed locally after its timestamp, even before that edit reaches `prefs`.
    let drafts = this.state.drafts
    for (const [guid, entry] of Object.entries(next)) {
      if (entry.draft === undefined || entry.draft === (drafts[guid] ?? '')) continue
      const current = drafts[guid] ?? ''
      if (current.length > 0 && (this.draftEditedAt.get(guid) ?? 0) >= (entry.updatedAt ?? 0)) continue
      drafts = { ...drafts, [guid]: entry.draft }
    }
    const changed = JSON.stringify(next) !== JSON.stringify(this.prefs)
    this.prefs = next
    if (changed) this.options.onPrefsChange?.(this.prefs)

    // An agent that predates this feature answers with no `gifs` field at all.
    const gifFavorites = mergeGifFavorites(this.state.gifFavorites, shared.gifs ?? {})
    const gifsChanged = JSON.stringify(gifFavorites) !== JSON.stringify(this.state.gifFavorites)
    if (gifsChanged) this.options.onGifFavoritesChange?.(gifFavorites)

    const patch: Partial<AppState> = {}
    if (changed) patch.chats = sortChats(this.state.chats.map((chat) => this.withPrefs(chat)))
    if (drafts !== this.state.drafts) patch.drafts = drafts
    if (gifsChanged) patch.gifFavorites = gifFavorites
    if (Object.keys(patch).length > 0) this.set(patch)
  }

  async deleteChat(chatGuid: string): Promise<void> {
    await this.transport.deleteChat(chatGuid)
    const chats = this.state.chats.filter((chat) => chat.guid !== chatGuid)
    this.set({ chats, selectedChat: this.state.selectedChat === chatGuid ? (chats[0]?.guid ?? null) : this.state.selectedChat })
  }

  /** Pages in the rest of the conversation's history, writes it to Markdown in Downloads, and opens the file. */
  async exportConversation(chatGuid: string): Promise<void> {
    const primary = conversationGuid(this.state, chatGuid)
    if (this.state.exportingChat) return
    this.set({ exportingChat: primary })
    try {
      while (conversationHasOlder(this.state, primary) && conversationMessages(this.state, primary).length < EXPORT_MAX_MESSAGES) {
        await this.loadEarlier(primary)
      }
      const chat = this.state.chats.find((item) => item.guid === primary)
      if (!chat) return
      const path = await writeConversationExport(chat, conversationHandles(this.state, primary), conversationMessages(this.state, primary))
      openExternal(path)
    } finally {
      this.set({ exportingChat: null })
    }
  }

  async createChat(addresses: string[], firstMessage: string): Promise<void> {
    let chat: Chat
    try {
      chat = await this.transport.createChat(addresses, firstMessage)
    } catch (error) {
      this.set({ error: errorText(error) })
      throw error
    }
    this.upsertChat(chat)
    await this.selectChat(chat.guid)
  }

  async renameGroup(chatGuid: string, name: string): Promise<void> {
    await this.transport.renameGroup(chatGuid, name)
    this.patchChat(chatGuid, { displayName: name })
  }

  async leaveGroup(chatGuid: string): Promise<void> {
    await this.transport.leaveGroup(chatGuid)
  }

  dismissFaceTime(): void {
    this.set({ facetime: null })
  }

  async answerFaceTime(): Promise<void> {
    const call = this.state.facetime
    if (!call || !call.canAnswer || call.status !== 'incoming') return
    this.set({ facetime: { ...call, status: 'answering' } })
    try {
      const link = await this.transport.answerFaceTime(call.callUuid)
      this.set({ facetime: { ...call, status: 'ready', link } })
      openExternal(link)
    } catch (error) {
      this.set({ facetime: { ...call, status: 'failed', error: errorText(error) } })
    }
  }

  async declineFaceTime(): Promise<void> {
    const call = this.state.facetime
    this.set({ facetime: null })
    if (call?.canAnswer) await this.transport.leaveFaceTime(call.callUuid).catch(() => undefined)
  }

  /** Creates a FaceTime Link, shares it in the chat and opens it here. Group links are the only calls a browser can join. */
  async startFaceTime(chatGuid: string): Promise<void> {
    try {
      const link = await this.transport.createFaceTimeLink()
      await this.send(chatGuid, link)
      openExternal(link)
    } catch (error) {
      this.set({ error: errorText(error) })
    }
  }

  clearError(): void {
    this.set({ error: undefined })
  }

  async attachmentSrc(chatGuid: string, messageGuid: string, attachmentGuid: string, name: string, mime?: string): Promise<string> {
    const local = await this.transport.attachmentPath(attachmentGuid, { name, mime })
    const target = this.findMessage(chatGuid, messageGuid)
    const current = target?.attachments.find((item) => item.guid === attachmentGuid)
    // chat.db often has no pixel size for an attachment; the file header does.
    const needsSize = Boolean(current && (!current.width || !current.height) && (mime ?? current.mime ?? '').startsWith('image/'))
    const size = needsSize ? await imageSize(local) : null
    const fresh = this.findMessage(chatGuid, messageGuid)
    if (fresh) {
      this.replaceMessage(chatGuid, {
        ...fresh,
        attachments: fresh.attachments.map((item) => (item.guid === attachmentGuid ? { ...item, localPath: local, ...(size ? { width: size.width, height: size.height } : {}) } : item)),
      })
    }
    return local
  }
}
