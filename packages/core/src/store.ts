import { openExternal } from './open'
import {
  capabilitiesFor,
  handleName,
  type Capabilities,
  type Chat,
  type Contact,
  type Message,
  type Reaction,
  type ServerInfo,
  type Tapback,
  type TapbackKind,
} from './model'
import type { ConnectionStatus, Transport, TransportEvent } from './transport'

export interface FaceTimeCall {
  callUuid: string
  from?: string
  status: 'incoming' | 'answering' | 'ready' | 'failed' | 'ended'
  canAnswer: boolean
  link?: string
  error?: string
}

export interface ChatPrefs {
  pinned?: boolean
  muted?: boolean
}

export interface AppState {
  status: ConnectionStatus
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
  /** Guid of the message being replied to in the composer, per chat. */
  replyingTo: Record<string, string | undefined>
  /** Guid of the message being edited in the composer, per chat. */
  editing: Record<string, string | undefined>
  facetime: FaceTimeCall | null
  lastSyncAt: number
}

export interface StoreOptions {
  prefs?: Record<string, ChatPrefs>
  onPrefsChange?: (prefs: Record<string, ChatPrefs>) => void
  onIncoming?: (chat: Chat, message: Message, target?: Message) => void
  pageSize?: number
  reconcileEveryMs?: number
}

const PAGE = 50
const TYPING_IDLE_MS = 3000

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

export class MessagesStore {
  state: AppState
  private listeners = new Set<() => void>()
  private prefs: Record<string, ChatPrefs>
  private options: StoreOptions
  private unsubscribe: (() => void) | null = null
  private pendingReactions = new Map<string, Reaction[]>()
  private typingTimers = new Map<string, ReturnType<typeof setTimeout>>()
  private typingSent = new Set<string>()
  private reconcileTimer: ReturnType<typeof setInterval> | null = null

  constructor(
    public readonly transport: Transport,
    options: StoreOptions = {},
  ) {
    this.options = options
    this.prefs = options.prefs ?? {}
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
      replyingTo: {},
      editing: {},
      facetime: null,
      lastSyncAt: Date.now(),
    }
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  getSnapshot = (): AppState => this.state

  private set(patch: Partial<AppState>): void {
    this.state = { ...this.state, ...patch }
    for (const listener of this.listeners) listener()
  }

  async start(): Promise<void> {
    this.unsubscribe = this.transport.subscribe((event) => this.handle(event))
    try {
      const info = await this.transport.connect()
      this.set({ server: info, capabilities: capabilitiesFor(info), error: undefined })
    } catch (error) {
      this.set({ status: 'offline', error: error instanceof Error ? error.message : String(error) })
      return
    }
    await this.refreshChats()
    const first = this.state.chats[0]
    if (first && !this.state.selectedChat) await this.selectChat(first.guid)
    void this.transport.listContacts().then((contacts) => this.set({ contacts })).catch(() => undefined)
    const every = this.options.reconcileEveryMs ?? 30_000
    if (every > 0) this.reconcileTimer = setInterval(() => void this.reconcile(), every)
  }

  stop(): void {
    this.unsubscribe?.()
    this.unsubscribe = null
    if (this.reconcileTimer) clearInterval(this.reconcileTimer)
    this.reconcileTimer = null
    for (const timer of this.typingTimers.values()) clearTimeout(timer)
    this.transport.disconnect()
  }

  private handle(event: TransportEvent): void {
    switch (event.type) {
      case 'connection': {
        const wasOffline = this.state.status !== 'online'
        this.set({ status: event.status, error: event.error })
        if (event.status === 'online' && wasOffline && this.state.chats.length > 0) void this.reconcile()
        return
      }
      case 'server':
        this.set({ server: event.info, capabilities: capabilitiesFor(event.info) })
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
        this.set({ typing: { ...this.state.typing, [event.chatGuid]: event.typing } })
        return
      case 'read':
        this.patchChat(event.chatGuid, { unread: !event.read })
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
  // thread are re-read, and messages created since the last pass are folded in.
  async reconcile(): Promise<void> {
    if (this.state.status !== 'online') return
    const since = this.state.lastSyncAt
    this.set({ lastSyncAt: Date.now() })
    try {
      await this.refreshChats()
      const recent = await this.transport.searchMessages('', { limit: 200, after: since })
      for (const message of recent) this.applyMessage(message, { fromServer: true, silent: true })
      const selected = this.state.selectedChat
      if (selected) {
        const page = await this.transport.loadMessages(selected, { limit: PAGE })
        for (const message of page.items) this.applyMessage(message, { fromServer: true, silent: true })
      }
    } catch (error) {
      console.error(`reconcile: ${String(error)}`)
    }
  }

  async refreshChats(): Promise<void> {
    const pageSize = 200
    const chats: Chat[] = []
    for (let offset = 0; offset < 5000; offset += pageSize) {
      const page = await this.transport.listChats({ limit: pageSize, offset })
      chats.push(...page.items.map((chat) => this.withPrefs(chat)))
      // The first page is enough to paint; the rest streams in behind it.
      if (offset === 0) this.set({ chats: sortChats(chats) })
      if (!page.hasMore) break
    }
    for (const chat of chats) {
      if (chat.lastMessage) this.stashReaction(chat.lastMessage)
    }
    this.set({ chats: sortChats(chats) })
  }

  private withPrefs(chat: Chat): Chat {
    const prefs = this.prefs[chat.guid]
    return { ...chat, pinned: prefs?.pinned ?? false, muted: prefs?.muted ?? false }
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

  async selectChat(chatGuid: string | null): Promise<void> {
    const previous = this.state.selectedChat
    if (previous && previous !== chatGuid) void this.stopTyping(previous)
    this.set({ selectedChat: chatGuid })
    if (!chatGuid) return
    if (!this.state.messages[chatGuid]) await this.loadOlder(chatGuid)
    const chat = this.state.chats.find((item) => item.guid === chatGuid)
    if (chat?.unread) void this.markRead(chatGuid)
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
      this.set({ error: error instanceof Error ? error.message : String(error) })
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

  private findMessage(chatGuid: string, guid: string): Message | undefined {
    return this.state.messages[chatGuid]?.find((message) => message.guid === guid)
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

  private replaceMessage(chatGuid: string, message: Message): void {
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
    const existingIndex = list.findIndex(
      (item) => item.guid === incoming.guid || (incoming.tempGuid && (item.guid === incoming.tempGuid || item.tempGuid === incoming.tempGuid)),
    )
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
      const selectedAndVisible = this.state.selectedChat === chatGuid
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
    if (isNew && !message.fromMe) this.set({ typing: { ...this.state.typing, [chatGuid]: false } })
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
    const chat = this.state.chats.find((item) => item.guid === chatGuid)
    const editingGuid = this.state.editing[chatGuid]
    this.set({ drafts: { ...this.state.drafts, [chatGuid]: '' }, editing: { ...this.state.editing, [chatGuid]: undefined } })
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
      service: chat?.service ?? 'iMessage',
      attachments: [],
      tapbacks: [],
      replyTo,
      effect: options.effect,
      isAudio: false,
    }
    this.applyMessage(optimistic)
    try {
      const sent = await this.transport.sendText(chatGuid, body, { replyTo, effect: options.effect, tempGuid })
      this.applyMessage({ ...sent, tempGuid })
    } catch (error) {
      this.applyMessage({ ...optimistic, error: error instanceof Error ? error.message : String(error) })
    }
  }

  async sendAttachment(chatGuid: string, path: string): Promise<void> {
    const chat = this.state.chats.find((item) => item.guid === chatGuid)
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
      service: chat?.service ?? 'iMessage',
      attachments: [{ guid: tempGuid, name, mime: file.type || 'application/octet-stream', bytes: file.size, isSticker: false, hidden: false, localPath: path }],
      tapbacks: [],
      isAudio: false,
    }
    this.applyMessage(optimistic)
    try {
      const sent = await this.transport.sendAttachment(chatGuid, path, { name, tempGuid })
      const attachments = sent.attachments.map((item) => ({ ...item, localPath: item.localPath ?? path }))
      this.applyMessage({ ...sent, attachments, tempGuid })
    } catch (error) {
      this.applyMessage({ ...optimistic, error: error instanceof Error ? error.message : String(error) })
    }
  }

  async retry(chatGuid: string, messageGuid: string): Promise<void> {
    const failed = this.findMessage(chatGuid, messageGuid)
    if (!failed?.error) return
    this.set({ messages: { ...this.state.messages, [chatGuid]: (this.state.messages[chatGuid] ?? []).filter((item) => item.guid !== messageGuid) } })
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
      await this.transport.react(chatGuid, messageGuid, kind, { emoji, remove })
    } catch (error) {
      this.replaceMessage(chatGuid, target)
      this.set({ error: error instanceof Error ? error.message : String(error) })
    }
  }

  async edit(chatGuid: string, messageGuid: string, text: string): Promise<void> {
    const target = this.findMessage(chatGuid, messageGuid)
    if (!target) return
    this.replaceMessage(chatGuid, { ...target, text, dateEdited: Date.now() })
    try {
      const updated = await this.transport.editMessage(chatGuid, messageGuid, text, { backwardsCompatText: `Edited to “${text}”` })
      this.applyMessage(updated)
    } catch (error) {
      this.replaceMessage(chatGuid, target)
      this.set({ error: error instanceof Error ? error.message : String(error) })
    }
  }

  async unsend(chatGuid: string, messageGuid: string): Promise<void> {
    const target = this.findMessage(chatGuid, messageGuid)
    if (!target) return
    this.replaceMessage(chatGuid, { ...target, dateRetracted: Date.now() })
    try {
      await this.transport.unsendMessage(chatGuid, messageGuid)
    } catch (error) {
      this.replaceMessage(chatGuid, target)
      this.set({ error: error instanceof Error ? error.message : String(error) })
    }
  }

  async markRead(chatGuid: string): Promise<void> {
    this.patchChat(chatGuid, { unread: false })
    if (!this.state.capabilities.readReceipts) return
    await this.transport.markRead(chatGuid).catch(() => undefined)
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

  private savePrefs(chatGuid: string, patch: ChatPrefs): void {
    this.prefs = { ...this.prefs, [chatGuid]: { ...this.prefs[chatGuid], ...patch } }
    this.options.onPrefsChange?.(this.prefs)
  }

  async deleteChat(chatGuid: string): Promise<void> {
    await this.transport.deleteChat(chatGuid)
    const chats = this.state.chats.filter((chat) => chat.guid !== chatGuid)
    this.set({ chats, selectedChat: this.state.selectedChat === chatGuid ? (chats[0]?.guid ?? null) : this.state.selectedChat })
  }

  async createChat(addresses: string[], firstMessage: string): Promise<void> {
    let chat: Chat
    try {
      chat = await this.transport.createChat(addresses, firstMessage)
    } catch (error) {
      this.set({ error: error instanceof Error ? error.message : String(error) })
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
      this.set({ facetime: { ...call, status: 'failed', error: error instanceof Error ? error.message : String(error) } })
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
      this.set({ error: error instanceof Error ? error.message : String(error) })
    }
  }

  clearError(): void {
    this.set({ error: undefined })
  }

  async attachmentSrc(chatGuid: string, messageGuid: string, attachmentGuid: string, name: string, mime?: string): Promise<string> {
    const local = await this.transport.attachmentPath(attachmentGuid, { name, mime })
    const target = this.findMessage(chatGuid, messageGuid)
    if (target) {
      this.replaceMessage(chatGuid, {
        ...target,
        attachments: target.attachments.map((item) => (item.guid === attachmentGuid ? { ...item, localPath: local } : item)),
      })
    }
    return local
  }
}
