import { mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { isPinned } from './agent'
import { conversationFocus } from './conversations'
import { StateCache } from './cache'
import { favoriteGifs, type Gif } from './gifs'
import type { Chat, Contact, FocusStatus, Message, ScheduledMessage, ServerInfo } from './model'
import { MessagesStore } from './store'
import { TransportError, type Page, type SearchFilters, type Transport, type TransportEvent } from './transport'

const info: ServerInfo = { version: 'test', macosVersion: '15.0', privateApi: false, helperConnected: false }

function chat(guid: string, lastActivity = 1000): Chat {
  return { guid, identifier: guid, service: 'iMessage', isGroup: false, participants: [{ address: guid, service: 'iMessage' }], pinned: false, muted: false, archived: false, unread: false, lastActivity }
}

function gif(id: string): Gif {
  return { id, previewUrl: `https://static.klipy.com/${id}/sm.gif`, gifUrl: `https://static.klipy.com/${id}/hd.gif`, width: 200, height: 200 }
}

function media(chatGuid: string, attachmentGuid: string, date: number, bytes: number, mime = 'image/jpeg'): Message {
  return { ...message(chatGuid, '', date), attachments: [{ guid: attachmentGuid, name: `file-${attachmentGuid}`, mime, bytes, isSticker: false, hidden: false }] }
}

/** Ten bytes is all `imageSize` reads from a GIF: the signature and the logical screen size. */
function gifDataUrl(width: number, height: number): string {
  const bytes = Uint8Array.from([...Buffer.from('GIF89a'), width & 0xff, width >> 8, height & 0xff, height >> 8])
  return `data:image/gif;base64,${Buffer.from(bytes).toString('base64')}`
}

let seq = 0
function message(chatGuid: string, text: string, date: number, fromMe = false): Message {
  seq += 1
  return { guid: `msg-${seq}`, chatGuid, text, fromMe, date, service: 'iMessage', attachments: [], tapbacks: [], isAudio: false }
}

/** Every failure the network can produce and every answer the server can give, on demand. */
class FakeTransport implements Transport {
  kind: Transport['kind'] = 'bluebubbles'
  private listeners = new Set<(event: TransportEvent) => void>()
  connectAttempts = 0
  failConnects = 0
  chats: Chat[] = []
  messages: Message[] = []
  /** A real image, as a data URL, for the tests where the store has to measure one. */
  attachmentSource?: string
  sendCalls: string[] = []
  sendFailures: Array<'network' | 'server'> = []
  searchCalls: number[] = []
  /** The server capabilities `connect()` reports; override before `start()` to test the private-API paths. */
  serverInfo: ServerInfo = info

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
    return this.serverInfo
  }

  disconnect(): void {}
  seedContacts(): void {}

  subscribe(listener: (event: TransportEvent) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  /** Like the real server, every row carries its newest message. */
  async listChats(): Promise<Page<Chat>> {
    const items = this.chats.map((chat) => {
      const newest = this.messages.filter((item) => item.chatGuid === chat.guid).sort((a, b) => b.date - a.date)[0]
      return newest ? { ...chat, lastMessage: newest } : chat
    })
    return { items, hasMore: false }
  }

  async getChat(chatGuid: string): Promise<Chat> {
    const found = this.chats.find((item) => item.guid === chatGuid)
    if (!found) throw new TransportError(404, 'no chat')
    return found
  }

  loadCalls: string[] = []
  async loadMessages(chatGuid: string, options: { limit: number; before?: number }): Promise<Page<Message>> {
    this.loadCalls.push(chatGuid)
    const all = this.messages.filter((item) => item.chatGuid === chatGuid && (options.before === undefined || item.date < options.before)).sort((a, b) => a.date - b.date)
    const items = all.slice(Math.max(0, all.length - options.limit))
    return { items, hasMore: items.length < all.length }
  }

  async searchMessages(_query: string, options: SearchFilters = {}): Promise<Message[]> {
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
  attachmentCalls: string[] = []
  async attachmentPath(attachmentGuid: string): Promise<string> {
    this.attachmentCalls.push(attachmentGuid)
    return this.attachmentSource ?? `/cache/${attachmentGuid}.jpg`
  }
  createChat(): Promise<Chat> {
    throw new Error('not in this test')
  }
  markReadCalls: string[] = []
  async markRead(chatGuid: string): Promise<void> {
    this.markReadCalls.push(chatGuid)
  }
  deleteFailure: Error | null = null
  async deleteChat(): Promise<void> {
    if (this.deleteFailure) throw this.deleteFailure
  }
  async react(): Promise<void> {}
  async setTyping(): Promise<void> {}
  async markUnread(): Promise<void> {}
  editMessage(): Promise<Message> {
    throw new Error('not in this test')
  }
  async unsendMessage(): Promise<void> {}
  renameFailure: Error | null = null
  async renameGroup(): Promise<void> {
    if (this.renameFailure) throw this.renameFailure
  }
  async addParticipant(): Promise<void> {}
  async removeParticipant(): Promise<void> {}
  async leaveGroup(): Promise<void> {}
  async setGroupIcon(): Promise<void> {}
  focusCalls: string[] = []
  focus: FocusStatus = 'none'
  async focusStatus(address: string): Promise<FocusStatus> {
    this.focusCalls.push(address)
    return this.focus
  }
  notifyCalls: string[] = []
  notifyFailure: Error | null = null
  async notifySilenced(_chatGuid: string, messageGuid: string): Promise<void> {
    this.notifyCalls.push(messageGuid)
    if (this.notifyFailure) throw this.notifyFailure
  }
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
  afterEach(() => vi.useRealTimers())

  it('pages the chats it has not opened in the background', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a', 2000), chat('b', 1000)]
    for (let index = 0; index < 60; index += 1) transport.messages.push(message('b', `older ${index}`, index + 1))
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()
    await settle()
    expect(store.state.selectedChat).toBe('a')
    expect(store.state.messages.b).toHaveLength(50)
    expect(store.state.hasOlder.b).toBe(true)
    store.stop()
  })

  it('downloads the media of recent messages in the background', async () => {
    vi.useFakeTimers()
    const transport = new FakeTransport()
    transport.chats = [chat('a', 2000), chat('b', 1000)]
    transport.messages.push(
      media('b', 'att-photo', 900, 1024),
      media('b', 'att-video', 910, 30 * 1024 * 1024, 'video/quicktime'),
      media('b', 'att-voice', 920, 20_000, 'audio/mp4'),
      // Too big for its kind, and a file that is not media at all: both wait
      // to be asked for, same as they would in the thread.
      media('b', 'att-huge', 930, 40 * 1024 * 1024),
      media('b', 'att-doc', 940, 1024, 'application/pdf'),
    )
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()
    await vi.advanceTimersByTimeAsync(5000)

    expect(transport.attachmentCalls).toEqual(['att-photo', 'att-video', 'att-voice'])
    expect(store.state.messages.b?.[0]?.attachments[0]?.localPath).toBe('/cache/att-photo.jpg')
    store.stop()
  })

  it('pages a chat the socket touched before it was opened', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a', 2000), chat('b', 1000)]
    for (let index = 0; index < 60; index += 1) transport.messages.push(message('b', `older ${index}`, index + 1))
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, warmChats: 0 })
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

  it('keeps chat and message identity when a reconcile brings back the same data', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    for (let index = 0; index < 5; index += 1) transport.messages.push(message('a', `row ${index}`, index + 1))
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()
    const chatsBefore = store.state.chats
    const rowBefore = store.state.messages.a
    // The server maps fresh objects every time; the store must not care.
    transport.chats = transport.chats.map((item) => ({ ...item }))
    transport.messages = transport.messages.map((item) => ({ ...item }))
    await store.reconcile()
    expect(store.state.chats).toBe(chatsBefore)
    expect(store.state.messages.a).toBe(rowBefore)
    store.stop()
  })

  it('applies a page as one publish rather than one per row', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a', 2000), chat('b', 1000)]
    for (let index = 0; index < 50; index += 1) transport.messages.push(message('b', `row ${index}`, index + 1))
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, warmChats: 0 })
    await store.start()
    let publishes = 0
    store.subscribe(() => {
      publishes += 1
    })

    await store.selectChat('b')
    expect(store.state.messages.b).toHaveLength(50)
    // The selection, the loading flag either side of the page, and the page:
    // a handful. It used to be one per row, and each one re-rendered the window.
    expect(publishes).toBeLessThan(10)
    store.stop()
  })

  it('trims a conversation once it is not the open one, leaving it pageable', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a', 2000), chat('b', 1000)]
    for (let index = 0; index < 260; index += 1) transport.messages.push(message('b', `row ${index}`, index + 1))
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, warmChats: 0 })
    await store.start()

    await store.selectChat('b')
    await store.loadEarlier('b')
    await store.loadEarlier('b')
    await store.loadEarlier('b')
    // What the open thread paged is what the reader is looking at, so it stays.
    expect(store.state.messages.b).toHaveLength(200)

    await store.selectChat('a')
    expect(store.state.messages.b).toHaveLength(100)
    expect(store.state.messages.b?.[99]?.text).toBe('row 259')
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
    expect(isPinned({ pinned: false, pinnedAt: 200, updatedAt: 200, macPinned: true, macPinnedAt: 100 })).toBe(false)
    expect(isPinned({ pinned: false, pinnedAt: 50, updatedAt: 50, macPinned: true, macPinnedAt: 100 })).toBe(true)
    expect(isPinned({ pinned: true, pinnedAt: 50, updatedAt: 50 })).toBe(true)
  })

  it('keeps a Mac pin when a draft or a mute writes to the same entry', () => {
    expect(isPinned({ draft: 'typing', updatedAt: 200, macPinned: true, macPinnedAt: 100 })).toBe(true)
    expect(isPinned({ muted: true, updatedAt: 200, macPinned: true, macPinnedAt: 100 })).toBe(true)
  })

  it('leaves a chat pinned in Messages.app pinned while it is typed in, and still unpins on request', async () => {
    vi.useFakeTimers()
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ chats: {}, gifs: {}, macPinned: ['a'], macPinnedAt: 100 }), { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, agent: { url: 'http://mac.local:1236', token: 'tok' } })
    await store.start()
    expect(store.state.chats[0]?.pinned).toBe(true)

    store.setDraft('a', 'on my way')
    await vi.advanceTimersByTimeAsync(2000)
    expect(store.state.chats[0]?.pinned).toBe(true)

    store.togglePin('a')
    expect(store.state.chats[0]?.pinned).toBe(false)
    store.stop()
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })
})

describe('attachments', () => {
  it('keeps the size read from the file header when the server sends the message again', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    const server = media('a', 'att-1', 5000, 1000, 'image/gif')
    // chat.db reports the pixels the way they are stored, so a portrait photo arrives as a landscape box.
    server.attachments = [{ ...server.attachments[0]!, width: 1200, height: 800 }]
    transport.messages = [server]
    transport.attachmentSource = gifDataUrl(240, 180)
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()
    await store.selectChat('a')
    await store.attachmentSrc('a', server.guid, 'att-1', 'clip.gif', 'image/gif')
    const shown = () => store.state.messages.a?.[0]?.attachments[0]
    expect(shown()).toMatchObject({ width: 240, height: 180, measured: true })

    store.applyMessage(server, { fromServer: true, silent: true })
    expect(shown()).toMatchObject({ width: 240, height: 180, localPath: transport.attachmentSource })
    store.stop()
  })
})

describe('read receipts', () => {
  it('turns them off without telling the server, and back on again', async () => {
    const transport = new FakeTransport()
    transport.serverInfo = { ...info, privateApi: true, helperConnected: true }
    transport.chats = [chat('a')]
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()

    store.toggleReadReceipts('a')
    expect(store.state.chats.find((item) => item.guid === 'a')?.readReceipts).toBe(false)
    await store.markUnread('a')
    await store.markRead('a')
    expect(store.state.chats.find((item) => item.guid === 'a')?.unread).toBe(false)
    expect(transport.markReadCalls).toEqual([])

    store.toggleReadReceipts('a')
    await store.markUnread('a')
    await store.markRead('a')
    expect(transport.markReadCalls).toEqual(['a'])
    store.stop()
  })
})

describe('restarting', () => {
  let dir = ''

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'messages-cache-'))
  })

  afterEach(async () => {
    vi.useRealTimers()
    // A debounced cache write can land while the directory is going away, which rmdir reports as ENOTEMPTY.
    await rm(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 20 })
  })

  /** Two chats worth thirty rows each, one of them carrying a photo. */
  function seeded(): FakeTransport {
    const transport = new FakeTransport()
    transport.chats = [chat('a', 2000), chat('b', 1000)]
    for (let index = 0; index < 30; index += 1) transport.messages.push(message('a', `a ${index}`, index + 1))
    for (let index = 0; index < 30; index += 1) transport.messages.push(message('b', `b ${index}`, index + 1))
    transport.messages.push(media('b', 'att-photo', 900, 1024))
    return transport
  }

  it('repairs an attachment an older build cached with no mime', async () => {
    // What an earlier `mime: string` wrote through when the server said null.
    const file = path.join(dir, 'state.json')
    await writeFile(
      file,
      JSON.stringify({
        version: 1,
        savedAt: Date.now(),
        selectedChat: 'a',
        chats: [chat('a')],
        contacts: [],
        messages: { a: [{ ...message('a', 'Your bill is ready', 1), attachments: [{ guid: 'brand-logo', name: 'BrandLogoImage', mime: null, bytes: 40_150, isSticker: false, hidden: false }] }] },
      }),
    )

    const loaded = await new StateCache(dir).load()
    const attachment = loaded?.messages.a?.[0]?.attachments[0]
    expect(attachment?.mime).toBe('application/octet-stream')
    expect(() => attachment!.mime.startsWith('image/')).not.toThrow()
  })

  it('comes back on the cache instead of pulling the threads and their media again', async () => {
    vi.useFakeTimers()
    const first = seeded()
    const cache = new StateCache(dir)
    const store = new MessagesStore(first, { reconcileEveryMs: 0, cache })
    await store.start()
    await vi.advanceTimersByTimeAsync(5000)
    expect(first.attachmentCalls).toEqual(['att-photo'])
    expect(store.state.messages.b).toHaveLength(31)
    await cache.flush()
    store.stop()

    const second = seeded()
    const restarted = new MessagesStore(second, { reconcileEveryMs: 0, cache: new StateCache(dir) })
    await restarted.start()
    await vi.advanceTimersByTimeAsync(5000)

    // Both threads came off disk, so the only page read is the reconcile's
    // safety net over the open one, and no attachment is fetched twice.
    expect(second.loadCalls).toEqual(['a'])
    expect(second.attachmentCalls).toEqual([])
    expect(restarted.state.messages.b).toHaveLength(31)
    expect(restarted.state.messages.b?.at(-1)?.attachments[0]?.localPath).toBe('/cache/att-photo.jpg')
    restarted.stop()
  })
})

describe('drafts', () => {
  const agentConfig = { url: 'http://mac.local:1236', token: 'tok' }

  afterEach(() => vi.unstubAllGlobals())

  function stubAgent(): ReturnType<typeof vi.fn> {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ chats: {}, macPinned: [], macPinnedAt: null, friends: [], updatedAt: 0 }), { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)
    return fetchMock
  }

  it('debounces a local edit 2s before syncing, and clears the synced draft once the message sends', async () => {
    vi.useFakeTimers()
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    const fetchMock = stubAgent()
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, agent: agentConfig })
    await store.start()
    fetchMock.mockClear()

    store.setDraft('a', 'hey there')
    expect(fetchMock).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(2000)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    const synced = JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))
    expect(synced.chats.a).toMatchObject({ draft: 'hey there' })

    await store.send('a', 'hey there')
    const cleared = JSON.parse(String(fetchMock.mock.calls.at(-1)?.[1]?.body))
    expect(cleared.chats.a).toMatchObject({ draft: '' })
    store.stop()
    vi.useRealTimers()
  })

  it('puts a newer remote draft into the composer when the local box is empty', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    const fetchMock = stubAgent()
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, agent: agentConfig })
    await store.start()

    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ chats: { a: { draft: 'from my phone', updatedAt: Date.now() } }, macPinned: [], macPinnedAt: null }), { status: 200 }))
    await store.syncPrefs()

    expect(store.state.drafts.a).toBe('from my phone')
    store.stop()
  })

  it('never lets a stale remote draft that arrives late overwrite text typed more recently', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    const fetchMock = stubAgent()
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, agent: agentConfig })
    await store.start()

    const editedAt = Date.now()
    store.setDraft('a', 'local text')
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ chats: { a: { draft: 'their text', updatedAt: editedAt - 1000 } }, macPinned: [], macPinnedAt: null }), { status: 200 }))
    await store.syncPrefs()

    expect(store.state.drafts.a).toBe('local text')
    store.stop()
  })
})

describe('gif favorites', () => {
  const agentConfig = { url: 'http://mac.local:1236', token: 'tok' }

  afterEach(() => vi.unstubAllGlobals())

  function stubAgent(): ReturnType<typeof vi.fn> {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ chats: {}, gifs: {}, macPinned: [], macPinnedAt: null }), { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)
    return fetchMock
  }

  it('favorites a gif and lists it newest first, then unfavorites it with a tombstone', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()

    store.toggleGifFavorite(gif('one'))
    await new Promise((resolve) => setTimeout(resolve, 1))
    store.toggleGifFavorite(gif('two'))
    expect(favoriteGifs(store.state.gifFavorites).map((item) => item.id)).toEqual(['two', 'one'])

    store.toggleGifFavorite(gif('one'))
    expect(favoriteGifs(store.state.gifFavorites).map((item) => item.id)).toEqual(['two'])
    expect(store.state.gifFavorites.one).toMatchObject({ removed: true })
    store.stop()
  })

  it('syncs favorites through the agent and merges a newer remote entry in', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    const fetchMock = stubAgent()
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, agent: agentConfig })
    await store.start()

    store.toggleGifFavorite(gif('one'))
    await new Promise((resolve) => setTimeout(resolve, 1))
    const synced = JSON.parse(String(fetchMock.mock.calls.at(-1)?.[1]?.body))
    expect(synced.gifs.one).toMatchObject({ gif: gif('one') })

    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify({ chats: {}, gifs: { two: { gif: gif('two'), updatedAt: Date.now() + 1000 } }, macPinned: [], macPinnedAt: null }), { status: 200 }),
    )
    await store.syncPrefs()

    expect(favoriteGifs(store.state.gifFavorites).map((item) => item.id)).toEqual(['two', 'one'])
    store.stop()
  })

  it('never lets a stale remote favorite overwrite a newer local unfavorite', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('a')]
    const fetchMock = stubAgent()
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, agent: agentConfig })
    await store.start()

    store.toggleGifFavorite(gif('one'))
    store.toggleGifFavorite(gif('one'))
    const localUpdatedAt = store.state.gifFavorites.one?.updatedAt ?? 0

    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify({ chats: {}, gifs: { one: { gif: gif('one'), updatedAt: localUpdatedAt - 500 } }, macPinned: [], macPinnedAt: null }), { status: 200 }),
    )
    await store.syncPrefs()

    expect(store.state.gifFavorites.one).toMatchObject({ removed: true, updatedAt: localUpdatedAt })
    store.stop()
  })
})

describe('chat actions', () => {
  async function online(chats: Chat[]): Promise<{ transport: FakeTransport; store: MessagesStore }> {
    const transport = new FakeTransport()
    transport.chats = chats
    const store = new MessagesStore(transport, { reconcileEveryMs: 0 })
    await store.start()
    return { transport, store }
  }

  it('drops a deleted conversation at once and brings it back when the server refuses', async () => {
    const { transport, store } = await online([chat('a', 2000), chat('b', 1000)])
    transport.deleteFailure = new TransportError(500, 'chat.db is locked')
    const pending = store.deleteChat('a')
    expect(store.state.chats.map((item) => item.guid)).toEqual(['b'])
    expect(store.state.selectedChat).toBe('b')
    await pending
    expect(store.state.chats.map((item) => item.guid)).toEqual(['a', 'b'])
    expect(store.state.error).toBe('chat.db is locked')
    store.stop()
  })

  it('renames a group before the server answers and reverts when it fails', async () => {
    const group: Chat = { ...chat('g'), isGroup: true, displayName: 'Old', participants: [{ address: 'x', service: 'iMessage' }, { address: 'y', service: 'iMessage' }] }
    const { transport, store } = await online([group])
    transport.renameFailure = new TransportError(400, 'the server said no')
    const pending = store.renameGroup('g', 'New')
    expect(store.state.chats[0]?.displayName).toBe('New')
    await pending
    expect(store.state.chats[0]?.displayName).toBe('Old')
    expect(store.state.error).toBe('the server said no')
    store.stop()
  })

  it('clears the unread dot the moment a conversation is selected', async () => {
    const { store } = await online([{ ...chat('a', 2000), unread: false }, { ...chat('b', 1000), unread: true }])
    const pending = store.selectChat('b')
    expect(store.state.chats.find((item) => item.guid === 'b')?.unread).toBe(false)
    await pending
    store.stop()
  })
})

describe('focus', () => {
  it('asks about the open conversation, keeps the answer, and breaks through it', async () => {
    const transport = new FakeTransport()
    transport.serverInfo = { ...info, privateApi: true, helperConnected: true }
    transport.chats = [chat('+15550101')]
    const mine = { ...message('+15550101', 'you up', 10, true), dateDelivered: 11, deliveredQuietly: true }
    transport.messages = [mine]
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, warmChats: 0 })
    transport.focus = 'silenced'
    await store.start()
    await settle()

    expect(transport.focusCalls).toEqual(['+15550101'])
    expect(conversationFocus(store.state, '+15550101')).toBe('silenced')

    // The TTL holds: reopening the same thread does not ask again.
    await store.selectChat('+15550101')
    expect(transport.focusCalls).toHaveLength(1)

    await store.notifySilenced('+15550101', mine.guid)
    expect(transport.notifyCalls).toEqual([mine.guid])
    expect(store.state.messages['+15550101']?.[0]?.notified).toBe(true)
    store.stop()
  })

  it('never asks when the private API is off, and puts the message back when the notify fails', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('+15550101')]
    const mine = { ...message('+15550101', 'you up', 10, true), dateDelivered: 11, deliveredQuietly: true }
    transport.messages = [mine]
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, warmChats: 0 })
    transport.focus = 'silenced'
    await store.start()
    await settle()

    expect(transport.focusCalls).toEqual([])
    expect(conversationFocus(store.state, '+15550101')).toBe('unknown')

    transport.notifyFailure = new Error('helper is gone')
    await store.notifySilenced('+15550101', mine.guid)
    expect(store.state.messages['+15550101']?.[0]?.notified).toBeUndefined()
    expect(store.state.error).toBe('helper is gone')
    store.stop()
  })
})

describe('find my', () => {
  const agentConfig = { url: 'http://mac.local:1236', token: 'tok' }

  afterEach(() => vi.unstubAllGlobals())

  function friend(latitude: number) {
    return { id: 'f1', addresses: ['+34600111222'], latitude, longitude: -15.4, timestamp: 1, isSharing: true }
  }

  /** Answers `/findmy/stream` with one pushed snapshot, and `/findmy/friends` (with the prefs sync) with `polled`. */
  function stubStream(pushed: unknown, polled: unknown[] = []): void {
    const encoder = new TextEncoder()
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string) => {
        if (String(url).endsWith('/findmy/stream')) {
          const body = new ReadableStream<Uint8Array>({
            start(controller) {
              controller.enqueue(encoder.encode(`data: ${JSON.stringify(pushed)}\n\n`))
            },
          })
          return { ok: true, status: 200, body } as unknown as Response
        }
        return new Response(JSON.stringify({ chats: {}, gifs: {}, macPinned: [], macPinnedAt: null, friends: polled, updatedAt: 0 }), { status: 200 })
      }),
    )
  }

  it('takes a pushed location while the details panel is open, without waiting for the poll', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('+34600111222')]
    stubStream({ friends: [friend(28.1)], devices: null, updatedAt: 5 })
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, warmChats: 0, agent: agentConfig })
    await store.start()

    store.setDetailsOpen(true)
    await vi.waitFor(() => expect(store.state.locations['600111222']?.latitude).toBe(28.1), { timeout: 5000 })
    expect(store.state.findMy).toBe('ok')

    store.setDetailsOpen(false)
    store.stop()
  })

  it('leaves the last known locations alone when a snapshot carries no friends', async () => {
    const transport = new FakeTransport()
    transport.chats = [chat('+34600111222')]
    stubStream({ friends: null, devices: [], updatedAt: 5 }, [friend(28.9)])
    const store = new MessagesStore(transport, { reconcileEveryMs: 0, warmChats: 0, agent: agentConfig })
    await store.start()
    await settle()
    expect(store.state.locations['600111222']?.latitude).toBe(28.9)

    store.setDetailsOpen(true)
    await settle()
    expect(store.state.locations['600111222']?.latitude).toBe(28.9)

    store.setDetailsOpen(false)
    store.stop()
  })
})
