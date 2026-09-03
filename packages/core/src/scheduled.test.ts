import { describe, expect, test } from 'vitest'
import { MessagesStore } from './store'
import type { Chat, Contact, Message, ScheduledMessage, ServerInfo } from './model'
import type { Page, SendAttachmentOptions, SendTextOptions, Transport, TransportEvent } from './transport'

const SERVER_INFO: ServerInfo = { version: 'fake', privateApi: true, helperConnected: true }

function makeChat(guid: string): Chat {
  return {
    guid,
    identifier: guid,
    service: 'iMessage',
    isGroup: false,
    participants: [],
    pinned: false,
    muted: false,
    archived: false,
    unread: false,
    lastActivity: Date.now(),
  }
}

/** Implements every Transport method with a stub; each test overrides only what it exercises. */
class FakeTransport implements Transport {
  seedContacts(): void {}
  readonly kind = 'demo' as const
  chats: Chat[] = []
  scheduled: ScheduledMessage[] = []
  private listeners = new Set<(event: TransportEvent) => void>()
  private scheduledSeq = 0

  subscribe(listener: (event: TransportEvent) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  async connect(): Promise<ServerInfo> {
    return SERVER_INFO
  }

  disconnect(): void {}

  async listChats(): Promise<Page<Chat>> {
    return { items: this.chats, hasMore: false }
  }

  async getChat(chatGuid: string): Promise<Chat> {
    const chat = this.chats.find((item) => item.guid === chatGuid)
    if (!chat) throw new Error(`No chat ${chatGuid}`)
    return chat
  }

  async loadMessages(): Promise<Page<Message>> {
    return { items: [], hasMore: false }
  }

  async searchMessages(): Promise<Message[]> {
    return []
  }

  async listContacts(): Promise<Contact[]> {
    return []
  }

  async sendText(chatGuid: string, text: string, options: SendTextOptions = {}): Promise<Message> {
    return {
      guid: `sent-${Date.now()}`,
      tempGuid: options.tempGuid,
      chatGuid,
      text,
      fromMe: true,
      date: Date.now(),
      service: 'iMessage',
      attachments: [],
      tapbacks: [],
      isAudio: false,
    }
  }

  async sendAttachment(chatGuid: string, _path: string, _options?: SendAttachmentOptions): Promise<Message> {
    throw new Error('not implemented')
  }

  async attachmentPath(): Promise<string> {
    throw new Error('not implemented')
  }

  async createChat(): Promise<Chat> {
    throw new Error('not implemented')
  }

  async markRead(): Promise<void> {}
  async deleteChat(): Promise<void> {}

  async scheduleText(chatGuid: string, text: string, sendAt: number): Promise<ScheduledMessage> {
    this.scheduledSeq += 1
    const message: ScheduledMessage = { id: `sched-${this.scheduledSeq}`, chatGuid, text, sendAt }
    this.scheduled.push(message)
    return message
  }

  // A copy, not the live array: the real transports never hand back a reference
  // the store could alias into its own state.
  async listScheduled(): Promise<ScheduledMessage[]> {
    return [...this.scheduled]
  }

  async cancelScheduled(id: string): Promise<void> {
    this.scheduled = this.scheduled.filter((item) => item.id !== id)
  }

  async react(): Promise<void> {}
  async setTyping(): Promise<void> {}
  async markUnread(): Promise<void> {}

  async editMessage(): Promise<Message> {
    throw new Error('not implemented')
  }

  async unsendMessage(): Promise<void> {}
  async renameGroup(): Promise<void> {}
  async addParticipant(): Promise<void> {}
  async removeParticipant(): Promise<void> {}
  async leaveGroup(): Promise<void> {}
  async setGroupIcon(): Promise<void> {}
  async notifySilenced(): Promise<void> {}

  async createFaceTimeLink(): Promise<string> {
    return ''
  }

  async answerFaceTime(): Promise<string> {
    return ''
  }

  async leaveFaceTime(): Promise<void> {}
}

function makeStore(transport: FakeTransport): MessagesStore {
  // reconcileEveryMs: 0 skips the interval timer, so a test does not leak one.
  return new MessagesStore(transport, { reconcileEveryMs: 0 })
}

describe('MessagesStore scheduled sends', () => {
  test('start refreshes the scheduled list from the transport', async () => {
    const transport = new FakeTransport()
    transport.chats = [makeChat('chat-1')]
    transport.scheduled = [{ id: 'sched-1', chatGuid: 'chat-1', text: 'later', sendAt: Date.now() + 60_000 }]
    const store = makeStore(transport)

    await store.start()

    expect(store.state.scheduled).toEqual(transport.scheduled)
  })

  test('scheduleSend adds the message and clears the draft, reply and edit state like send() does', async () => {
    const transport = new FakeTransport()
    transport.chats = [makeChat('chat-1')]
    const store = makeStore(transport)
    await store.start()
    store.setDraft('chat-1', 'see you then')
    store.setReplyingTo('chat-1', 'msg-1')

    await store.scheduleSend('chat-1', 'see you then', Date.now() + 3_600_000)

    expect(store.state.drafts['chat-1']).toBe('')
    expect(store.state.replyingTo['chat-1']).toBeUndefined()
    expect(store.state.editing['chat-1']).toBeUndefined()
    expect(store.state.scheduled).toHaveLength(1)
    expect(store.state.scheduled[0]?.text).toBe('see you then')
  })

  test('scheduleSend ignores an empty draft', async () => {
    const transport = new FakeTransport()
    transport.chats = [makeChat('chat-1')]
    const store = makeStore(transport)
    await store.start()

    await store.scheduleSend('chat-1', '   ', Date.now() + 3_600_000)

    expect(store.state.scheduled).toHaveLength(0)
  })

  test('cancelScheduled removes the message optimistically and keeps it gone on success', async () => {
    const transport = new FakeTransport()
    transport.chats = [makeChat('chat-1')]
    const store = makeStore(transport)
    await store.start()
    await store.scheduleSend('chat-1', 'later', Date.now() + 3_600_000)
    const id = store.state.scheduled[0]!.id

    await store.cancelScheduled(id)

    expect(store.state.scheduled).toHaveLength(0)
    expect(transport.scheduled).toHaveLength(0)
  })

  test('cancelScheduled restores the message if the transport call fails', async () => {
    const transport = new FakeTransport()
    transport.chats = [makeChat('chat-1')]
    const store = makeStore(transport)
    await store.start()
    await store.scheduleSend('chat-1', 'later', Date.now() + 3_600_000)
    const id = store.state.scheduled[0]!.id
    transport.cancelScheduled = async () => {
      throw new Error('offline')
    }

    await store.cancelScheduled(id)

    expect(store.state.scheduled).toHaveLength(1)
    expect(store.state.error).toBe('offline')
  })
})
