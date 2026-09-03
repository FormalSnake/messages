import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { isPinned } from './agent'
import type { Chat, Contact, Message, ScheduledMessage, ServerInfo } from './model'
import { MessagesStore } from './store'
import { TransportError, type Page, type Transport, type TransportEvent } from './transport'

const info: ServerInfo = { version: 'test', macosVersion: '15.0', privateApi: false, helperConnected: false }

function chat(guid: string, lastActivity = 1000): Chat {
  return { guid, identifier: guid, service: 'iMessage', isGroup: false, participants: [{ address: guid, service: 'iMessage' }], pinned: false, muted: false, archived: false, unread: false, lastActivity }
}

let seq = 0
function message(chatGuid: string, text: string, date: number, fromMe = false): Message {
  seq += 1
  return { guid: `msg-${seq}`, chatGuid, text, fromMe, date, service: 'iMessage', attachments: [], tapbacks: [], isAudio: false }
}

/** Every failure the network can produce and every answer the server can give, on demand. */
class FakeTransport implements Transport {
  readonly kind = 'demo' as const
  private listeners = new Set<(event: TransportEvent) => void>()
  connectAttempts = 0
  failConnects = 0
  chats: Chat[] = []
  messages: Message[] = []
  sendCalls: string[] = []
  sendFailures: Array<'network' | 'server'> = []
  searchCalls: number[] = []

  emit(event: TransportEvent): void {
    for (const listener of this.listeners) listener(event)
  }

  async connect(): Promise<ServerInfo> {
    this.connectAttempts += 1
    if (this.failConnects > 0) {
      this.failConnects -= 1
      this.emit({ type: 'connection', status: 'offline', error: 'fetch failed' })
      throw new TypeError('fetch failed')
    }
    this.emit({ type: 'connection', status: 'online' })
    return info
  }

  disconnect(): void {}
  seedContacts(): void {}

  subscribe(listener: (event: TransportEvent) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  async listChats(): Promise<Page<Chat>> {
    return { items: this.chats, hasMore: false }
  }

  async getChat(chatGuid: string): Promise<Chat> {
    const found = this.chats.find((item) => item.guid === chatGuid)
    if (!found) throw new TransportError(404, 'no chat')
    return found
  }

  async loadMessages(chatGuid: string, options: { limit: number; before?: number }): Promise<Page<Message>> {
    const all = this.messages.filter((item) => item.chatGuid === chatGuid && (options.before === undefined || item.date < options.before)).sort((a, b) => a.date - b.date)
    const items = all.slice(Math.max(0, all.length - options.limit))
    return { items, hasMore: items.length < all.length }
  }

  async searchMessages(_query: string, options: { chatGuid?: string; limit?: number; after?: number } = {}): Promise<Message[]> {
    const limit = options.limit ?? 50
    this.searchCalls.push(limit)
    return this.messages
      .filter((item) => options.after === undefined || item.date > options.after)
      .sort((a, b) => a.date - b.date)
      .slice(0, limit)
  }

  async listContacts(): Promise<Contact[]> {
    return []
  }

  async sendText(chatGuid: string, text: string): Promise<Message> {
    this.sendCalls.push(text)
    const failure = this.sendFailures.shift()
    if (failure === 'network') throw new TypeError('fetch failed')
    if (failure === 'server') throw new TransportError(400, 'the server said no')
    const sent = message(chatGuid, text, Date.now(), true)
    this.messages.push(sent)
    return sent
  }

  sendAttachment(): Promise<Message> {
    throw new Error('not in this test')
  }
  attachmentPath(): Promise<string> {
    throw new Error('not in this test')
  }
  createChat(): Promise<Chat> {
    throw new Error('not in this test')
  }
  async markRead(): Promise<void> {}
  async deleteChat(): Promise<void> {}
  async react(): Promise<void> {}
  async setTyping(): Promise<void> {}
  async markUnread(): Promise<void> {}
  editMessage(): Promise<Message> {
    throw new Error('not in this test')
  }
  async unsendMessage(): Promise<void> {}
  async renameGroup(): Promise<void> {}
  async addParticipant(): Promise<void> {}
  async removeParticipant(): Promise<void> {}
  async leaveGroup(): Promise<void> {}
  async setGroupIcon(): Promise<void> {}
  async notifySilenced(): Promise<void> {}
  createFaceTimeLink(): Promise<string> {
    throw new Error('not in this test')
  }
  answerFaceTime(): Promise<string> {
    throw new Error('not in this test')
  }
  async leaveFaceTime(): Promise<void> {}
  scheduleText(): Promise<ScheduledMessage> {
    throw new Error('not in this test')
  }
  async listScheduled(): Promise<ScheduledMessage[]> {
    return []
  }
  async cancelScheduled(): Promise<void> {}
}

async function settle(): Promise<void> {
  for (let index = 0; index < 10; index += 1) await Promise.resolve()
}

describe('connecting', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('keeps retrying with backoff until the server answers', async () => {
    const transport = new FakeTransport()
    transport.failConnects = 2
    transport.chats = [chat('a')]
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    const started = store.start()
    await settle()
    expect(store.state.status).toBe('offline')
    expect(store.state.connectionError).toBe('fetch failed')
    await vi.advanceTimersByTimeAsync(2000)
    expect(transport.connectAttempts).toBe(2)
    await vi.advanceTimersByTimeAsync(4000)
    await started
    expect(transport.connectAttempts).toBe(3)
    expect(store.state.status).toBe('online')
    expect(store.state.connectionError).toBeUndefined()
    expect(store.state.selectedChat).toBe('a')
    store.stop()
  })
})

describe('sending', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  async function online(): Promise<{ transport: FakeTransport; store: MessagesStore }> {
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()
    return { transport, store }
  }

  it('queues sends while offline and flushes them in order when the connection is back', async () => {
    const { transport, store } = await online()
    transport.emit({ type: 'connection', status: 'offline', error: 'transport close' })
    await store.send('a', 'first')
    await store.send('a', 'second')
    await settle()
    expect(transport.sendCalls).toEqual([])
    expect(store.pendingSends).toBe(2)
    expect(store.state.messages.a?.map((item) => item.text)).toEqual(['first', 'second'])
    expect(store.state.messages.a?.every((item) => item.guid === item.tempGuid)).toBe(true)

    transport.emit({ type: 'connection', status: 'online' })
    await settle()
    expect(transport.sendCalls).toEqual(['first', 'second'])
    expect(store.pendingSends).toBe(0)
    expect(store.state.messages.a?.map((item) => item.guid.startsWith('msg-'))).toEqual([true, true])
    store.stop()
  })

  it('folds the socket echo of a send into the optimistic row when the echo has no temp guid', async () => {
    const { transport, store } = await online()
    transport.sendFailures = ['network']
    await store.send('a', 'photo caption')
    await settle()
    const echo = message('a', 'photo caption', Date.now(), true)
    transport.emit({ type: 'message', message: echo })
    expect(store.state.messages.a?.map((item) => item.guid)).toEqual([echo.guid])
    await vi.advanceTimersByTimeAsync(2000)
    expect(transport.sendCalls).toEqual(['photo caption'])
    expect(store.pendingSends).toBe(0)
    store.stop()
  })

  it('retries a send the network dropped and fails one the server refused', async () => {
    const { transport, store } = await online()
    transport.sendFailures = ['network']
    await store.send('a', 'flaky')
    await settle()
    expect(transport.sendCalls).toEqual(['flaky'])
    expect(store.state.messages.a?.[0]?.error).toBeUndefined()
    await vi.advanceTimersByTimeAsync(2000)
    expect(transport.sendCalls).toEqual(['flaky', 'flaky'])
    expect(store.state.messages.a?.[0]?.guid.startsWith('msg-')).toBe(true)

    transport.sendFailures = ['server']
    await store.send('a', 'refused')
    await settle()
    const refused = store.state.messages.a?.find((item) => item.text === 'refused')
    expect(refused?.error).toBe('the server said no')
    expect(store.pendingSends).toBe(0)
    store.stop()
  })
})

describe('reading', () => {
  it('pages a chat the socket touched before it was opened', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a', 2000), chat('b', 1000)]
    for (let index = 0; index < 60; index += 1) transport.messages.push(message('b', `older ${index}`, index + 1))
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()
    expect(store.state.selectedChat).toBe('a')

    const fresh = message('b', 'just arrived', 5000)
    transport.messages.push(fresh)
    transport.emit({ type: 'message', message: fresh })
    expect(store.state.messages.b).toHaveLength(1)

    await store.selectChat('b')
    expect(store.state.messages.b).toHaveLength(51)
    expect(store.state.messages.b?.[50]?.text).toBe('just arrived')
    expect(store.state.hasOlder.b).toBe(true)
    store.stop()
  })

  it('catches up in pages of ten', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()
    transport.searchCalls = []
    const base = store.state.lastSyncAt
    for (let index = 0; index < 25; index += 1) transport.messages.push(message('a', `missed ${index}`, base + index + 1))
    await new Promise((resolve) => setTimeout(resolve, 30))

    await store.reconcile()
    expect(transport.searchCalls).toEqual([10, 10, 10])
    expect(store.state.messages.a?.filter((item) => item.text.startsWith('missed'))).toHaveLength(25)
    expect(store.state.lastSyncAt).toBeGreaterThan(base + 25)
    store.stop()
  })
})

describe('conversations', () => {
  it('folds two chats with the same person into one, replies to the newer one and reads both', async () => {
    const transport = new FakeTransport()
    const phone = chat('any;-;+32470000001', 5000)
    const email = chat('any;-;papa@example.com', 1000)
    phone.participants = [{ address: '+32470000001', service: 'iMessage' }]
    email.participants = [{ address: 'papa@example.com', service: 'iMessage' }]
    email.unread = true
    transport.chats = [phone, email, chat('c', 100)]
    transport.messages.push(message('any;-;papa@example.com', 'from the email', 900), message('any;-;+32470000001', 'from the phone', 4900))
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()
    transport.emit({ type: 'contacts', contacts: [{ id: 'papa', name: 'Papa', addresses: ['+32 470 00 00 01', 'papa@example.com'] }] })

    expect(store.state.merged[phone.guid]).toEqual([phone.guid, email.guid])
    expect(store.state.primaryOf[email.guid]).toBe(phone.guid)
    await store.selectChat(email.guid)
    expect(store.state.selectedChat).toBe(phone.guid)
    const { conversationMessages, conversationChats } = await import('./conversations')
    expect(conversationMessages(store.state, phone.guid).map((item) => item.text)).toEqual(['from the email', 'from the phone'])
    expect(conversationChats(store.state).map((item) => item.guid)).toEqual([phone.guid, 'c'])
    expect(store.state.chats.find((item) => item.guid === email.guid)?.unread).toBe(false)

    await store.send(phone.guid, 'hello')
    await settle()
    expect(transport.messages[transport.messages.length - 1]?.chatGuid).toBe(phone.guid)
    store.stop()
  })
})

describe('pins', () => {
  it('lets a newer client change win over a Mac pin, and the Mac win over an older one', () => {
    expect(isPinned(undefined)).toBe(false)
    expect(isPinned({ pinned: true })).toBe(true)
    expect(isPinned({ macPinned: true, macPinnedAt: 100 })).toBe(true)
    expect(isPinned({ pinned: false, updatedAt: 200, macPinned: true, macPinnedAt: 100 })).toBe(false)
    expect(isPinned({ pinned: false, updatedAt: 50, macPinned: true, macPinnedAt: 100 })).toBe(true)
    expect(isPinned({ pinned: true, updatedAt: 50 })).toBe(true)
  })
})
